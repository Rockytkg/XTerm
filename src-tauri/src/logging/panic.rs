use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;

use super::retention::{cap_emergency_file, EMERGENCY_FILE_MAX_BYTES};

/// Panic details are appended here, bypassing the (possibly broken or
/// unflushed) fern pipeline so a crash always leaves on-disk evidence.
pub const PANIC_LOG_FILE: &str = "panic.log";
/// Startup failures that happen before the log target exists land here.
pub const STARTUP_ERROR_LOG_FILE: &str = "startup-error.log";

/// The log directory becomes known only after `AppPaths::initialize`
/// succeeds; the panic hook is installed before that and picks the directory
/// up from here once set.
static PANIC_LOG_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Installs the process panic hook exactly once, as early as possible. The
/// hook appends the panic message to `<log_dir>/panic.log` with a plain `fs`
/// append — deliberately bypassing the fern pipeline, which may be the thing
/// that panicked — and also reports through `log::error!`. Until
/// [`set_panic_log_dir`] runs, the file append is skipped but the
/// `log::error!` path still works.
pub fn install_panic_hook() {
    static HOOK_ONCE: std::sync::Once = std::sync::Once::new();
    HOOK_ONCE.call_once(|| {
        std::panic::set_hook(Box::new(move |info| {
            let message = format!("panic: {info}");
            if let Some(log_dir) = PANIC_LOG_DIR.get() {
                append_emergency_line(log_dir, PANIC_LOG_FILE, &message);
            }
            log::error!("{message}");
        }));
    });
}

pub fn set_panic_log_dir(log_dir: PathBuf) {
    let _ = PANIC_LOG_DIR.set(log_dir);
}

/// Best-effort append of one line to a log file that must work even when the
/// regular logging pipeline is unavailable (startup failure, panic). The
/// file is tail-capped first so repeated crashes cannot grow it without
/// bound. All errors are ignored on purpose: callers are already on a
/// failure path.
pub fn append_emergency_line(dir: &Path, file_name: &str, line: &str) {
    let path = dir.join(file_name);
    cap_emergency_file(&path, EMERGENCY_FILE_MAX_BYTES);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let _ = writeln!(file, "[{timestamp}] {line}");
}
