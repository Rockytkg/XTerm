//! Platform-aware persistence for the main window placement.
//!
//! Windows must use `WINDOWPLACEMENT` instead of restoring a client-area size
//! through Tao. Tao intentionally adds the hidden non-client insets for
//! undecorated windows with shadows; applying that conversion to a size that
//! was already measured from the client area causes the size to grow after
//! every restart. `WINDOWPLACEMENT` keeps the native frame, shadow and corner
//! calculations entirely in the platform window manager.

#[cfg(not(windows))]
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_window_state::StateFlags;

    tauri_plugin_window_state::Builder::default()
        .with_state_flags(
            StateFlags::SIZE
                | StateFlags::POSITION
                | StateFlags::MAXIMIZED
                | StateFlags::FULLSCREEN,
        )
        .build()
}

#[cfg(windows)]
mod windows_impl {
    use std::{
        collections::HashMap,
        fs,
        mem::size_of,
        os::windows::ffi::OsStrExt,
        path::Path,
        sync::{Arc, Mutex},
    };

    use serde::{Deserialize, Serialize};
    use tauri::{
        plugin::{Builder, TauriPlugin},
        AppHandle, Manager, RunEvent, Runtime, Window, WindowEvent,
    };
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            MONITOR_DEFAULTTONULL,
        },
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
        UI::WindowsAndMessaging::{
            GetClientRect, GetWindowPlacement, GetWindowRect, SetWindowPlacement, ShowWindow,
            SW_HIDE, SW_SHOWMAXIMIZED, SW_SHOWNORMAL, WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
        },
    };

    const MAIN_WINDOW_LABEL: &str = "main";
    const STATE_FILENAME: &str = ".xterm-window-placement.json";
    const LEGACY_STATE_FILENAME: &str = ".window-state.json";
    const STATE_VERSION: u32 = 1;

    #[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
    struct WindowRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl From<RECT> for WindowRect {
        fn from(rect: RECT) -> Self {
            Self {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            }
        }
    }

    impl From<WindowRect> for RECT {
        fn from(rect: WindowRect) -> Self {
            Self {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            }
        }
    }

    impl WindowRect {
        fn width(self) -> i32 {
            self.right.saturating_sub(self.left)
        }

        fn height(self) -> i32 {
            self.bottom.saturating_sub(self.top)
        }

        fn is_valid(self) -> bool {
            self.width() > 0 && self.height() > 0
        }
    }

    #[derive(Clone, Copy, Debug, Deserialize, Serialize)]
    struct WindowPlacementState {
        normal_rect: WindowRect,
        maximized: bool,
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct StateFile {
        version: u32,
        windows: HashMap<String, WindowPlacementState>,
    }

    #[derive(Default)]
    struct Cache {
        windows: HashMap<String, WindowPlacementState>,
    }

    #[derive(Default, Deserialize)]
    struct LegacyRoot {
        main: Option<LegacyMainWindow>,
    }

    #[derive(Default, Deserialize)]
    struct LegacyMainWindow {
        width: Option<f64>,
        height: Option<f64>,
        x: Option<f64>,
        y: Option<f64>,
        position: Option<LegacyPosition>,
        maximized: Option<bool>,
    }

    #[derive(Default, Deserialize)]
    struct LegacyPosition {
        x: Option<f64>,
        y: Option<f64>,
    }

    pub(super) fn plugin<R: Runtime>() -> TauriPlugin<R> {
        Builder::new("xterm-window-state")
            .setup(|app, _api| {
                let windows = load_state_file(app).unwrap_or_default();
                app.manage(Arc::new(Mutex::new(Cache { windows })));
                Ok(())
            })
            .on_window_ready(|window| {
                if window.label() != MAIN_WINDOW_LABEL {
                    return;
                }

                restore_window(&window);

                let cache = window
                    .app_handle()
                    .state::<Arc<Mutex<Cache>>>()
                    .inner()
                    .clone();
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if matches!(event, WindowEvent::CloseRequested { .. }) {
                        update_cached_state(&window_clone, &cache);
                    }
                });
            })
            .on_event(|app, event| {
                if matches!(event, RunEvent::Exit) {
                    save_state_file(app);
                }
            })
            .build()
    }

    fn load_state_file<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Result<HashMap<String, WindowPlacementState>, String> {
        let path = app
            .path()
            .app_config_dir()
            .map_err(|error| error.to_string())?
            .join(STATE_FILENAME);

        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<StateFile>(&bytes)
                .map(|file| file.windows)
                .map_err(|error| format!("invalid window placement state: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
            Err(error) => Err(format!("failed to read window placement state: {error}")),
        }
    }

    fn save_state_file<R: Runtime>(app: &AppHandle<R>) {
        let Some(webview) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
            return;
        };
        let window = webview.as_ref().window();

        let cache = app.state::<Arc<Mutex<Cache>>>();
        update_cached_state(&window, cache.inner());

        let path = match app.path().app_config_dir() {
            Ok(dir) => dir.join(STATE_FILENAME),
            Err(error) => {
                log::warn!(target: "app.window_state", "failed to resolve state directory: {error}");
                return;
            }
        };

        let state = {
            let cache = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            StateFile {
                version: STATE_VERSION,
                windows: cache.windows.clone(),
            }
        };

        if let Err(error) = write_json(&path, &state) {
            log::warn!(target: "app.window_state", "failed to save window placement: {error}");
        }
    }

    fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "window state path has no parent".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        replace_file(&temporary, path)
    }

    fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
        let source = wide_path(source);
        let destination = wide_path(destination);
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;

        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn update_cached_state<R: Runtime>(window: &Window<R>, cache: &Arc<Mutex<Cache>>) {
        let Some(state) = read_window_state(window) else {
            return;
        };
        let mut cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.windows.insert(MAIN_WINDOW_LABEL.to_string(), state);
    }

    fn read_window_state<R: Runtime>(window: &Window<R>) -> Option<WindowPlacementState> {
        let hwnd = native_hwnd(window)?;
        let mut placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };

        if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
            return None;
        }

        let normal_rect = WindowRect::from(placement.rcNormalPosition);
        normal_rect.is_valid().then_some(WindowPlacementState {
            normal_rect,
            maximized: placement.showCmd == SW_SHOWMAXIMIZED as u32
                || placement.flags & WPF_RESTORETOMAXIMIZED != 0,
        })
    }

    fn restore_window<R: Runtime>(window: &Window<R>) {
        let Some(hwnd) = native_hwnd(window) else {
            return;
        };

        let app = window.app_handle();
        let cache = app.state::<Arc<Mutex<Cache>>>();
        let mut state = {
            let cache = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            cache.windows.get(MAIN_WINDOW_LABEL).copied()
        };

        if state.is_none() {
            if let Some(legacy_state) = load_legacy_state(window) {
                let mut cache = cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                cache
                    .windows
                    .insert(MAIN_WINDOW_LABEL.to_string(), legacy_state);
                state = Some(legacy_state);
            }
        }

        let Some(mut state) = state else {
            return;
        };

        state.normal_rect = keep_rect_visible(state.normal_rect);
        let placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            flags: 0,
            showCmd: if state.maximized {
                SW_SHOWMAXIMIZED as u32
            } else {
                SW_SHOWNORMAL as u32
            },
            rcNormalPosition: state.normal_rect.into(),
            ..Default::default()
        };

        if unsafe { SetWindowPlacement(hwnd, &placement) } == 0 {
            log::warn!(target: "app.window_state", "failed to restore native window placement");
        } else {
            // SetWindowPlacement applies showCmd as well as the normal rect; a
            // SW_SHOWNORMAL / SW_SHOWMAXIMIZED command can reveal the window
            // before the frontend has finished its first-frame startup gate.
            // Keep the restored placement (including maximized state), but
            // leave the actual reveal to src/main.js after UI initialization.
            unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    fn load_legacy_state<R: Runtime>(window: &Window<R>) -> Option<WindowPlacementState> {
        let path = window
            .app_handle()
            .path()
            .app_config_dir()
            .ok()?
            .join(LEGACY_STATE_FILENAME);
        let bytes = fs::read(path).ok()?;
        let root = serde_json::from_slice::<LegacyRoot>(&bytes).ok()?;
        let main = root.main?;
        let width = round_dimension(main.width?)?;
        let height = round_dimension(main.height?)?;
        let position = main.position;
        let x = round_coordinate(position.as_ref().and_then(|p| p.x).or(main.x)?)?;
        let y = round_coordinate(position.as_ref().and_then(|p| p.y).or(main.y)?)?;

        let (width_offset, height_offset) = native_frame_offsets(window)?;
        let normal_rect = WindowRect {
            left: x,
            top: y,
            right: x.saturating_add(width).saturating_add(width_offset),
            bottom: y.saturating_add(height).saturating_add(height_offset),
        };

        normal_rect.is_valid().then_some(WindowPlacementState {
            normal_rect,
            maximized: main.maximized.unwrap_or(false),
        })
    }

    fn native_frame_offsets<R: Runtime>(window: &Window<R>) -> Option<(i32, i32)> {
        let hwnd = native_hwnd(window)?;
        let mut outer = RECT::default();
        let mut client = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut outer) } == 0
            || unsafe { GetClientRect(hwnd, &mut client) } == 0
        {
            return None;
        }
        Some((
            (outer.right - outer.left) - (client.right - client.left),
            (outer.bottom - outer.top) - (client.bottom - client.top),
        ))
    }

    fn round_dimension(value: f64) -> Option<i32> {
        if value.is_finite() && value > 0.0 && value <= i32::MAX as f64 {
            Some(value.round() as i32)
        } else {
            None
        }
    }

    fn round_coordinate(value: f64) -> Option<i32> {
        if value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64 {
            Some(value.round() as i32)
        } else {
            None
        }
    }

    fn keep_rect_visible(rect: WindowRect) -> WindowRect {
        if !rect.is_valid() {
            return rect;
        }

        let native_rect: RECT = rect.into();
        let monitor = unsafe { MonitorFromRect(&native_rect, MONITOR_DEFAULTTONULL) };
        if !monitor.is_null() {
            return rect;
        }

        let monitor = unsafe { MonitorFromRect(&native_rect, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return rect;
        }

        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return rect;
        }

        let width = rect.width();
        let height = rect.height();
        let work_width = info.rcWork.right.saturating_sub(info.rcWork.left);
        let work_height = info.rcWork.bottom.saturating_sub(info.rcWork.top);
        let left = if width >= work_width {
            info.rcWork.left
        } else {
            rect.left
                .clamp(info.rcWork.left, info.rcWork.right.saturating_sub(width))
        };
        let top = if height >= work_height {
            info.rcWork.top
        } else {
            rect.top
                .clamp(info.rcWork.top, info.rcWork.bottom.saturating_sub(height))
        };

        WindowRect {
            left,
            top,
            right: left.saturating_add(width),
            bottom: top.saturating_add(height),
        }
    }

    fn native_hwnd<R: Runtime>(window: &Window<R>) -> Option<HWND> {
        window.hwnd().ok().map(|hwnd| hwnd.0 as HWND)
    }
}

#[cfg(windows)]
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    windows_impl::plugin()
}
