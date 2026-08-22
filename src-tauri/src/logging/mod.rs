//! Application logging system.
//!
//! Architecture:
//! - **Levels**: persisted in the settings store (`logLevel`), loaded once at
//!   startup ([`persisted_log_level`]) into the shared runtime level
//!   ([`set_active_level`] / [`active_level`]). `log::set_max_level` gates
//!   records at the call site; the runtime level additionally drives the
//!   per-crate clamp inside the daily-file dispatch, so changing the level
//!   via `log_level_set` takes effect immediately — including raising it,
//!   which the plugin's static `level_for` could not do.
//! - **Sinks**: one daily-rotated file (`<log_dir>/YYYYMMDD.log`, built by
//!   [`daily_log_target`]) plus the plugin's Webview target. Writes are
//!   unbuffered and flushed per record so a crash never loses recent lines.
//! - **Retention**: daily files older than 7 days (or beyond 14 files) are
//!   pruned on startup and at midnight rotation; `panic.log` /
//!   `startup-error.log` are tail-capped at 4 MiB before each append.
//! - **Conventions**: structured events go through [`event`] with a dotted
//!   logical scope (`terminal.connection_service`) mirroring the frontend
//!   logger scopes (`frontend.*`); plain messages use `log::<level>!` with an
//!   explicit `target:` scope. Bare macros without a target only remain in
//!   the panic/startup emergency paths, which must not depend on this module.

pub(crate) mod commands;
mod event;
mod level;
mod panic;
mod retention;
mod writer;

pub(crate) use event::{event, LogEvent};
pub use level::{active_level, persisted_log_level, set_active_level};
pub use panic::{
    append_emergency_line, install_panic_hook, set_panic_log_dir, STARTUP_ERROR_LOG_FILE,
};
pub use writer::daily_log_target;

#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique temporary directory per test so cases never share state.
    pub(crate) fn temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xterm-logging-test-{}-{}-{}",
            name,
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("failed to create test directory");
        dir
    }
}
