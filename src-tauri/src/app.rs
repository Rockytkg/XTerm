use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use log::LevelFilter;
use tauri::{Manager, WindowEvent};
use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};

use crate::{paths::AppPaths, state::AppState, storage::Store};

const MAIN_WINDOW_LABEL: &str = "main";
const CONTEXT_MENU_WINDOW_LABEL: &str = "context-menu";

static APP_SHUTDOWN_STARTED: AtomicBool = AtomicBool::new(false);

// ── Windows single-instance with WM_COPYDATA deep-link forwarding ────────
#[cfg(windows)]
mod single_instance {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::app::MAIN_WINDOW_LABEL;
    use tauri::{Emitter, Manager};

    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
        },
        System::Threading::CreateMutexW,
        UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        UI::WindowsAndMessaging::{
            EnumWindows, GetPropW, IsWindow, RemovePropW, SendMessageW, SetForegroundWindow,
            SetPropW, ShowWindow, SW_RESTORE, SW_SHOW, WM_COPYDATA, WM_NCDESTROY,
        },
    };

    const MUTEX_NAME: &str = "Global\\com.liushicong.xterm.single_instance";
    const WINDOW_PROP_NAME: &str = "com.liushicong.xterm.single_instance.main_hwnd";
    const WINDOW_PROP_VALUE: usize = 1;
    const WINDOW_LOOKUP_ATTEMPTS: usize = 60;
    const WINDOW_LOOKUP_RETRY_DELAY: Duration = Duration::from_millis(50);
    const DEEPLINK_SUBCLASS_ID: usize = 1;
    /// Magic value placed in `COPYDATASTRUCT.dwData` so the receiver can
    /// distinguish our payloads from unrelated WM_COPYDATA messages.
    const DEEPLINK_COPYDATA_MAGIC: usize = 0x4465_6570_4C69_6E6B; // "DeepLink"

    /// Minimal `COPYDATASTRUCT` — avoids per-crate binding differences.
    /// Layout matches the Win32 definition exactly.
    #[repr(C)]
    struct CopyData {
        dw_data: usize,
        cb_data: u32,
        lp_data: *const u8,
    }

    struct SubclassState {
        tx: mpsc::Sender<String>,
        prop_name: Vec<u16>,
    }

    struct WindowSearch {
        prop_name: *const u16,
        hwnd: HWND,
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    /// Encode `s` as a null-terminated UTF-16 vector.
    fn wide_nullterm(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Decode null-terminated UTF-16LE bytes. Returns `None` when the
    /// content is not valid UTF-16.
    fn decode_utf16le_nullterm(bytes: &[u8]) -> Option<String> {
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&w| w != 0)
            .collect();
        String::from_utf16(&words).ok()
    }

    fn is_supported_deeplink(url: &str) -> bool {
        url.starts_with("ssh://") || url.starts_with("telnet://")
    }

    // ── Second-instance guard ────────────────────────────────────────────

    /// Returns `true` if another instance is already running.
    ///
    /// When a duplicate launch carries deep-link URLs they are forwarded
    /// via `WM_COPYDATA`, the existing window is focused, and the caller
    /// should exit.
    pub fn ensure_single_instance() -> bool {
        let name = wide_nullterm(MUTEX_NAME);

        unsafe {
            let hmutex: HANDLE = CreateMutexW(ptr::null(), 1, name.as_ptr());

            if hmutex.is_null() {
                log::error!(
                    target: "app.single_instance",
                    "CreateMutexW returned NULL (error={}). \
                     Single-instance guard is inactive.",
                    GetLastError()
                );
                return false;
            }

            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(hmutex);
                handle_second_instance();
                return true;
            }

            // First instance — leak handle to keep mutex alive.
            let _ = Box::leak(Box::new(hmutex));
            false
        }
    }

    /// Locate the existing window, focus it, then forward any deep-link
    /// URLs extracted from our command line.
    fn handle_second_instance() {
        let Some(hwnd) = find_existing_main_window() else {
            log::warn!(
                target: "app.single_instance",
                "single-instance mutex existed but no marked main window was found"
            );
            return;
        };

        unsafe {
            // Focus first for immediate visual feedback. SendMessageW with
            // WM_COPYDATA is synchronous so the user would otherwise see a delay.
            log::info!(
                target: "app.single_instance",
                "found existing XTerm window (hwnd={hwnd:?}), focusing"
            );
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }

        forward_urls(hwnd);
    }

    fn find_existing_main_window() -> Option<HWND> {
        let prop_name = wide_nullterm(WINDOW_PROP_NAME);

        for _ in 0..WINDOW_LOOKUP_ATTEMPTS {
            let mut search = WindowSearch {
                prop_name: prop_name.as_ptr(),
                hwnd: ptr::null_mut(),
            };

            unsafe {
                EnumWindows(
                    Some(enum_windows_for_instance),
                    &mut search as *mut _ as LPARAM,
                );
            }

            if !search.hwnd.is_null() {
                return Some(search.hwnd);
            }

            thread::sleep(WINDOW_LOOKUP_RETRY_DELAY);
        }

        None
    }

    unsafe extern "system" fn enum_windows_for_instance(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = unsafe { &mut *(lparam as *mut WindowSearch) };
        let prop = unsafe { GetPropW(hwnd, search.prop_name) };

        if prop == WINDOW_PROP_VALUE as HANDLE && unsafe { IsWindow(hwnd) } != 0 {
            search.hwnd = hwnd;
            return 0;
        }

        1
    }

    fn forward_urls(hwnd: HWND) {
        for arg in std::env::args().skip(1) {
            for url in arg
                .split_whitespace()
                .filter(|url| is_supported_deeplink(url))
            {
                send_url(hwnd, url);
            }
        }
    }

    fn send_url(hwnd: HWND, url: &str) {
        let wide = wide_nullterm(url);
        let cds = CopyData {
            dw_data: DEEPLINK_COPYDATA_MAGIC,
            cb_data: (wide.len() * 2) as u32,
            lp_data: wide.as_ptr() as *const u8,
        };

        let result =
            unsafe { SendMessageW(hwnd, WM_COPYDATA, 0, &cds as *const CopyData as isize) };

        if result != 0 {
            log::info!(
                target: "app.deep_link",
                "deep-link forward: WM_COPYDATA accepted ({url})"
            );
        } else {
            log::warn!(
                target: "app.deep_link",
                "deep-link forward: WM_COPYDATA not processed ({url})"
            );
        }
    }

    fn install_window_marker(hwnd: HWND) -> Option<Vec<u16>> {
        let prop_name = wide_nullterm(WINDOW_PROP_NAME);

        if unsafe { SetPropW(hwnd, prop_name.as_ptr(), WINDOW_PROP_VALUE as HANDLE) } == 0 {
            log::error!(target: "app.single_instance", "single-instance marker: SetPropW failed");
            return None;
        }

        Some(prop_name)
    }

    fn remove_window_marker(hwnd: HWND, prop_name: &[u16]) {
        unsafe {
            RemovePropW(hwnd, prop_name.as_ptr());
        }
    }

    fn install_subclass(hwnd: HWND, state_ptr: *mut SubclassState) -> bool {
        let ok = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(subclass_proc),
                DEEPLINK_SUBCLASS_ID,
                state_ptr as usize,
            )
        };

        if ok == 0 {
            log::error!(
                target: "app.deep_link",
                "deep-link subclass: SetWindowSubclass failed"
            );
            return false;
        }

        true
    }

    fn spawn_deeplink_forwarder<R: tauri::Runtime>(
        app_handle: tauri::AppHandle<R>,
        rx: mpsc::Receiver<String>,
    ) {
        std::thread::spawn(move || {
            for url in rx {
                log::info!(target: "app.deep_link", "deep-link: received forwarded URL: {url}");
                let _ = app_handle.emit("deep-link://new-url", vec![url]);
            }
            log::debug!(target: "app.deep_link", "deep-link forwarder thread exiting");
        });
    }

    // ── First-instance subclass (WM_COPYDATA receiver) ───────────────────

    /// Install a `WM_COPYDATA` handler on the main window via
    /// [`SetWindowSubclass`].  Forwarded URLs are relayed to the Tauri
    /// deep-link plugin through the `deep-link://new-url` event.
    pub fn install_deeplink_subclass<R: tauri::Runtime>(app_handle: tauri::AppHandle<R>) {
        let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) else {
            log::error!(target: "app.deep_link", "deep-link subclass: main window not found");
            return;
        };

        let Ok(h) = window.hwnd() else {
            log::error!(target: "app.deep_link", "deep-link subclass: failed to get HWND");
            return;
        };
        let hwnd = h.0 as HWND;

        let Some(prop_name) = install_window_marker(hwnd) else {
            return;
        };

        let (tx, rx) = mpsc::channel::<String>();
        let state_ptr = Box::into_raw(Box::new(SubclassState { tx, prop_name }));

        if !install_subclass(hwnd, state_ptr) {
            unsafe {
                let state = Box::from_raw(state_ptr);
                remove_window_marker(hwnd, &state.prop_name);
            }
            return;
        }

        log::info!(target: "app.deep_link", "deep-link subclass installed");
        spawn_deeplink_forwarder(app_handle, rx);
    }

    /// Subclass window procedure — runs on the UI thread.
    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
        _uid_subclass: usize,
        dw_ref_data: usize,
    ) -> LRESULT {
        if msg == WM_COPYDATA {
            let state = unsafe { &*(dw_ref_data as *const SubclassState) };
            let cds = unsafe { &*(lparam as *const CopyData) };

            // Validate magic so we don't process unrelated WM_COPYDATA.
            if cds.dw_data == DEEPLINK_COPYDATA_MAGIC && cds.cb_data > 0 && !cds.lp_data.is_null() {
                let bytes =
                    unsafe { std::slice::from_raw_parts(cds.lp_data, cds.cb_data as usize) };
                if let Some(url) = decode_utf16le_nullterm(bytes) {
                    if is_supported_deeplink(&url) {
                        let _ = state.tx.send(url);
                        return 1; // processed — non-zero = success
                    }
                }
            }
        } else if msg == WM_NCDESTROY {
            // WM_NCDESTROY — window is being destroyed.  Recover and drop
            // the sender (which signals the background thread to exit),
            // remove our window marker, then remove the subclass.
            unsafe {
                let state = Box::from_raw(dw_ref_data as *mut SubclassState);
                remove_window_marker(hwnd, &state.prop_name);
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), DEEPLINK_SUBCLASS_ID);
            }
        }

        DefSubclassProc(hwnd, msg, _wparam, lparam)
    }
}

#[tauri::command]
pub(crate) fn open_devtools(window: tauri::WebviewWindow) -> Result<(), String> {
    window.open_devtools();
    Ok(())
}

pub fn run() {
    #[cfg(windows)]
    {
        if single_instance::ensure_single_instance() {
            std::process::exit(0);
        }
    }

    // Force the Tauri async runtime to exist before the log plugin installs
    // the global logger. The plugin's Webview target forwards every record by
    // spawning onto this runtime; creating the runtime lazily from inside the
    // logging path emits Trace records (mio's poll registry) that re-enter the
    // runtime OnceLock and deadlock. If that happens, plugin initialization
    // never finishes and the main window is never created or shown.
    tauri::async_runtime::spawn(async {});

    let paths = match AppPaths::initialize() {
        Ok(paths) => paths,
        Err(error) => startup_fatal(
            None,
            &format!("failed to initialize application data directories: {error}"),
        ),
    };
    let log_dir = paths.log_dir().to_path_buf();
    crate::logging::set_panic_log_dir(log_dir.clone());

    let store = match Store::open(paths.data_dir()) {
        Ok(store) => store,
        // The redb file lock is released when a process exits, so this error
        // means another instance is alive and owns the window. The single-
        // instance plugin would only detect it after the store opens, so exit
        // quietly here instead of showing a fatal startup dialog.
        Err(error) if error.contains("Cannot acquire lock") => {
            log::info!(
                target: "app.lifecycle",
                "another XTerm instance is already running; exiting"
            );
            std::process::exit(0);
        }
        Err(error) => startup_fatal(
            Some(&log_dir),
            &format!("failed to initialize application store: {error}"),
        ),
    };

    let log_level = crate::logging::persisted_log_level(&store);
    crate::logging::set_active_level(log_level);
    let log_target = match crate::logging::daily_log_target(log_dir.clone()) {
        Ok(target) => target,
        Err(error) => startup_fatal(
            Some(&log_dir),
            &format!("failed to initialize daily log file target: {error}"),
        ),
    };

    let builder = tauri::Builder::default()
        .manage(AppState::new(store, paths))
        .plugin(
            // The root dispatch stays at Trace so it never blocks records;
            // the effective gate is `log::set_max_level` (set from the
            // persisted level in `.setup` and updated by `log_level_set`)
            // plus the per-crate clamp inside the daily-file dispatch.
            tauri_plugin_log::Builder::new()
                .targets([log_target, Target::new(TargetKind::Webview)])
                .level(LevelFilter::Trace)
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .build(),
        )
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&[CONTEXT_MENU_WINDOW_LABEL])
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(not(windows))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        log::info!(
            target: "app.single_instance",
            "second launch detected with {} argument(s)",
            args.len()
        );
        restore_main_window(app);
    }));

    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(crate::command_registry::app_invoke_handler!());

    if let Err(error) = configure_builder(builder).run(tauri::generate_context!()) {
        crate::logging::append_emergency_line(
            &log_dir,
            crate::logging::STARTUP_ERROR_LOG_FILE,
            &format!("error while running tauri application: {error}"),
        );
        log::error!(
            target: "app.lifecycle",
            "error while running tauri application: {error}"
        );
        std::process::exit(1);
    }
}

/// Handles a startup failure that prevents the Tauri app from being built.
/// The reason is always appended to `startup-error.log` (falling back to a
/// best-effort log directory when the configured one is unavailable), a
/// native error dialog is shown where the platform allows it, and the
/// process exits with a non-zero code.
fn startup_fatal(log_dir: Option<&Path>, message: &str) -> ! {
    let dir = log_dir
        .map(Path::to_path_buf)
        .or_else(fallback_startup_log_dir);
    if let Some(dir) = &dir {
        crate::logging::append_emergency_line(dir, crate::logging::STARTUP_ERROR_LOG_FILE, message);
    }
    log::error!(target: "app.lifecycle", "fatal startup error: {message}");
    show_startup_error_dialog(message, dir.as_deref());
    std::process::exit(1);
}

/// Last-resort log directory used when `AppPaths::initialize` itself failed.
/// Mirrors the same base-directory resolution (executable directory when
/// writable, otherwise the OS per-user data directory) so the emergency log
/// does not target the directory that just failed.
fn fallback_startup_log_dir() -> Option<PathBuf> {
    let dir = crate::paths::fallback_base_dir()?.join("data").join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(windows)]
fn show_startup_error_dialog(message: &str, log_dir: Option<&Path>) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text = match log_dir {
        Some(dir) => format!(
            "XTerm failed to start.\n\n{message}\n\nDetails were written to:\n{}",
            dir.join(crate::logging::STARTUP_ERROR_LOG_FILE)
                .to_string_lossy()
        ),
        None => format!("XTerm failed to start.\n\n{message}"),
    };
    let wide = |value: &str| -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let text = wide(&text);
    let caption = wide("XTerm startup error");
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_startup_error_dialog(message: &str, log_dir: Option<&Path>) {
    let text = match log_dir {
        Some(dir) => format!(
            "XTerm failed to start.\n\n{message}\n\nDetails were written to:\n{}",
            dir.join(crate::logging::STARTUP_ERROR_LOG_FILE)
                .to_string_lossy()
        ),
        None => format!("XTerm failed to start.\n\n{message}"),
    };
    eprintln!("{text}");
    show_desktop_error_dialog(&text);
}

/// Best-effort native error dialog for the pre-app startup phase, where the
/// Tauri dialog plugin is not available yet. Failure just means the user
/// only sees the stderr/log output.
#[cfg(target_os = "macos")]
fn show_desktop_error_dialog(text: &str) {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"display dialog "{escaped}" with title "XTerm startup error" buttons {{"OK"}} default button "OK" with icon stop"#
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status();
}

#[cfg(target_os = "linux")]
fn show_desktop_error_dialog(text: &str) {
    let attempts: [(&str, Vec<&str>); 2] = [
        (
            "zenity",
            vec!["--error", "--title=XTerm startup error", "--text", text],
        ),
        (
            "kdialog",
            vec!["--title", "XTerm startup error", "--error", text],
        ),
    ];
    for (program, args) in attempts {
        let Ok(status) = std::process::Command::new(program).args(&args).status() else {
            continue;
        };
        if status.success() {
            return;
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn show_desktop_error_dialog(_text: &str) {}

fn configure_builder<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    #[cfg(feature = "mcp-bridge")]
    let builder = builder.plugin(
        tauri_plugin_mcp_bridge::Builder::new()
            .bind_address("127.0.0.1")
            .build(),
    );

    builder
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                if APP_SHUTDOWN_STARTED.swap(true, Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                begin_application_shutdown(window.app_handle().clone());
            }
        })
        .setup(|app| {
            // The log plugin attached itself with max level Trace; restore
            // the persisted level as the real call-site gate.
            log::set_max_level(crate::logging::active_level());

            #[cfg(windows)]
            single_instance::install_deeplink_subclass(app.handle().clone());

            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                configure_webview_permissions(&window);
                let _ = window.set_background_color(Some(tauri::window::Color(247, 248, 250, 255)));
            }
            log::info!(target: "app.lifecycle", "XTerm application started");
            Ok(())
        })
}

fn begin_application_shutdown<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        shutdown_application_resources(&app).await;
    });
}

async fn shutdown_application_resources<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    crate::terminal::shutdown_all_sessions(app);
    crate::proxy::shutdown_proxy(app).await;
    crate::file_service::shutdown_runtime(app).await;
    if let Some(window) = app.get_webview_window(CONTEXT_MENU_WINDOW_LABEL) {
        let _ = window.close();
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.close();
    }
}

#[cfg(not(windows))]
fn restore_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::UserAttentionType;

    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        log::warn!(target: "app.single_instance", "single-instance: main window not found");
        return;
    };

    let mut errors: Vec<String> = Vec::new();

    if let Err(e) = window.show() {
        errors.push(format!("show: {e}"));
    }
    if let Err(e) = window.unminimize() {
        errors.push(format!("unminimize: {e}"));
    }
    if let Err(e) = window.set_focus() {
        errors.push(format!("focus: {e}"));
        let _ = window.request_user_attention(Some(UserAttentionType::Critical));
    }

    if !errors.is_empty() {
        log::warn!(
            target: "app.single_instance",
            "single-instance window restore: {}",
            errors.join("; ")
        );
    } else {
        log::info!(target: "app.single_instance", "single-instance: main window restored");
    }
}

#[cfg(windows)]
fn configure_webview_permissions<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.with_webview(|webview| {
        use webview2_com::{
            Microsoft::Web::WebView2::Win32::{
                ICoreWebView2Settings3, COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS,
                COREWEBVIEW2_PERMISSION_STATE_ALLOW,
            },
            PermissionRequestedEventHandler,
        };
        use windows_core::Interface;

        let Ok(webview) = (unsafe { webview.controller().CoreWebView2() }) else {
            return;
        };

        match unsafe { webview.Settings() } {
            Ok(settings) => match settings.cast::<ICoreWebView2Settings3>() {
                Ok(settings3) => {
                    if let Err(error) =
                        unsafe { settings3.SetAreBrowserAcceleratorKeysEnabled(false) }
                    {
                        log::warn!(
                            target: "app.webview",
                            "failed to disable WebView2 browser accelerator keys: {error}"
                        );
                    }
                }
                Err(error) => {
                    log::warn!(
                        target: "app.webview",
                        "WebView2 browser accelerator key settings are unavailable: {error}"
                    );
                }
            },
            Err(error) => {
                log::warn!(
                    target: "app.webview",
                    "failed to access WebView2 settings: {error}"
                );
            }
        }

        let handler = PermissionRequestedEventHandler::create(Box::new(|_sender, args| {
            if let Some(args) = args {
                let mut permission_kind = Default::default();
                unsafe {
                    args.PermissionKind(&mut permission_kind)?;
                    if permission_kind == COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                    }
                }
            }
            Ok(())
        }));

        let mut token = 0;
        if let Err(error) = unsafe { webview.add_PermissionRequested(&handler, &mut token) } {
            log::warn!(
                target: "app.webview",
                "failed to configure WebView2 permission handling: {error}"
            );
        }
    });
}

#[cfg(not(windows))]
fn configure_webview_permissions<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}
