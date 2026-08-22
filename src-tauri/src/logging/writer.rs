use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;
use log::{Level, LevelFilter};
use tauri_plugin_log::{fern, Target, TargetKind};

use super::{level::active_level, retention::prune_daily_logs};

/// Builds the daily-file log target. The wrapped fern dispatch carries a
/// filter that clamps noisy dependency crates (`russh`, `keyring`, `mio`,
/// `tao`) to a floor level unless the user explicitly raised the runtime
/// level to Debug/Trace — replacing the plugin's static `level_for`, which
/// cannot react to runtime level changes.
pub fn daily_log_target(log_dir: PathBuf) -> Result<Target, String> {
    let writer = DailyLogWriter::new(log_dir)?;
    let dispatch = fern::Dispatch::new()
        .filter(noisy_crate_allowed)
        .chain(fern::Output::writer(Box::new(writer), "\n"));
    Ok(Target::new(TargetKind::Dispatch(dispatch)))
}

fn noisy_crate_allowed(metadata: &log::Metadata) -> bool {
    if active_level() >= LevelFilter::Debug {
        return true;
    }
    let target = metadata.target();
    let floor = if target.starts_with("russh")
        || target.starts_with("keyring")
        || target.starts_with("mio")
    {
        Level::Warn
    } else if target.starts_with("tao") {
        Level::Info
    } else {
        return true;
    };
    metadata.level() <= floor
}

/// Writes records to `<dir>/<yyyymmdd>.log`, rotating at local midnight.
/// Writes go straight to the OS without in-process buffering and every
/// record is flushed, so a crash never strands recent lines in a userspace
/// buffer (the previous `BufWriter` could hold back 8 KiB indefinitely).
struct DailyLogWriter {
    dir: PathBuf,
    current_date: String,
    file: File,
}

impl DailyLogWriter {
    fn new(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir)
            .map_err(|error| format!("failed to create log directory: {error}"))?;
        prune_non_fatal(&dir);
        let current_date = Local::now().format("%Y%m%d").to_string();
        let file = Self::open_file(&dir, &current_date)?;
        Ok(Self {
            dir,
            current_date,
            file,
        })
    }

    fn refresh_file_if_needed(&mut self) -> std::io::Result<()> {
        let date = Local::now().format("%Y%m%d").to_string();
        if date == self.current_date {
            return Ok(());
        }
        self.file.flush()?;
        self.file = Self::open_file(&self.dir, &date).map_err(std::io::Error::other)?;
        self.current_date = date;
        prune_non_fatal(&self.dir);
        Ok(())
    }

    fn open_file(dir: &Path, date: &str) -> Result<File, String> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("{date}.log")))
            .map_err(|error| format!("failed to open daily log file: {error}"))
    }
}

/// Retention cleanup must never take logging (or startup) down with it.
fn prune_non_fatal(dir: &Path) {
    if let Err(error) = prune_daily_logs(dir) {
        log::warn!(target: "logging.retention", "failed to prune old log files: {error}");
    }
}

impl Write for DailyLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.refresh_file_if_needed()?;
        let written = self.file.write(buf)?;
        self.file.flush()?;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::test_support::temp_test_dir;

    #[test]
    fn writes_are_visible_without_flush_or_close() {
        let dir = temp_test_dir("writer-unbuffered");
        let mut writer = DailyLogWriter::new(dir.clone()).expect("writer init failed");
        writer.write_all(b"hello log\n").expect("write failed");
        // Read through a second handle while the writer is still open: the
        // record must already be on disk (no userspace buffering).
        let date = Local::now().format("%Y%m%d").to_string();
        let content = fs::read_to_string(dir.join(format!("{date}.log"))).unwrap();
        assert!(content.contains("hello log"));
    }

    #[test]
    fn noisy_crate_clamp_follows_active_level() {
        use log::{Level, MetadataBuilder};

        let russh_info = MetadataBuilder::new()
            .target("russh::connection")
            .level(Level::Info)
            .build();
        let app_info = MetadataBuilder::new()
            .target("terminal.session_service")
            .level(Level::Info)
            .build();
        let tao_debug = MetadataBuilder::new()
            .target("tao::platform")
            .level(Level::Debug)
            .build();

        crate::logging::set_active_level(LevelFilter::Info);
        assert!(!noisy_crate_allowed(&russh_info));
        assert!(noisy_crate_allowed(&app_info));
        assert!(!noisy_crate_allowed(&tao_debug));

        crate::logging::set_active_level(LevelFilter::Debug);
        assert!(noisy_crate_allowed(&russh_info));
        assert!(noisy_crate_allowed(&tao_debug));

        crate::logging::set_active_level(LevelFilter::Info);
    }
}
