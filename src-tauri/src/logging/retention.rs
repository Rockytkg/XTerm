use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use chrono::{Duration, Local, NaiveDate};

/// Daily log files older than this many days are removed.
pub(crate) const DAILY_LOG_RETENTION_DAYS: i64 = 7;
/// Upper bound on kept daily files, applied on top of the date rule so a
/// clock skew or a flood of rotated files cannot fill the disk.
pub(crate) const DAILY_LOG_MAX_FILES: usize = 14;
/// `panic.log` / `startup-error.log` are append-only emergency files; they
/// are truncated to their tail once they exceed this size.
pub(crate) const EMERGENCY_FILE_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Extracts the date from a daily log file name (`YYYYMMDD.log`).
pub(crate) fn daily_log_date(name: &str) -> Option<NaiveDate> {
    let stem = name.strip_suffix(".log")?;
    if stem.len() == 8 && stem.starts_with("20") && stem.bytes().all(|byte| byte.is_ascii_digit()) {
        NaiveDate::parse_from_str(stem, "%Y%m%d").ok()
    } else {
        None
    }
}

fn daily_log_files(dir: &Path) -> Result<Vec<(NaiveDate, PathBuf)>, String> {
    let mut logs = fs::read_dir(dir)
        .map_err(|error| format!("failed to read log directory: {error}"))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.path().is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let date = daily_log_date(&name)?;
            Some((date, entry.path()))
        })
        .collect::<Vec<_>>();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(logs)
}

/// Deletes daily log files outside the retention window. Returns the number
/// of removed files. Emergency files (`panic.log`, `startup-error.log`) do
/// not match the daily pattern and are never touched here.
pub fn prune_daily_logs(dir: &Path) -> Result<usize, String> {
    let today = Local::now().date_naive();
    let oldest_kept = today - Duration::days(DAILY_LOG_RETENTION_DAYS);
    let mut removed = 0usize;
    let mut errors = Vec::new();

    for (index, (date, path)) in daily_log_files(dir)?.into_iter().enumerate() {
        if index < DAILY_LOG_MAX_FILES && date >= oldest_kept {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(error) => errors.push(format!("{}: {error}", path.to_string_lossy())),
        }
    }

    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(format!(
            "failed to remove old log file(s): {}",
            errors.join("; ")
        ))
    }
}

/// Keeps an append-only emergency log under `max_bytes` by rewriting it with
/// its tail. Best-effort: callers are already on a failure path, so all
/// errors are ignored on purpose.
pub(crate) fn cap_emergency_file(path: &Path, max_bytes: u64) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() <= max_bytes {
        return;
    }
    // Keep only half the budget worth of tail so the file does not sit right
    // at the cap after the next append.
    let keep = max_bytes / 2;
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    let mut tail = Vec::new();
    if file.seek(SeekFrom::End(-(keep as i64))).is_err() || file.read_to_end(&mut tail).is_err() {
        return;
    }
    // Start at the next line boundary so the kept tail does not begin with a
    // partial line; fall back to the raw tail for files without newlines.
    if let Some(position) = tail.iter().position(|byte| *byte == b'\n') {
        tail.drain(..=position);
    }
    if let Ok(mut file) = fs::File::create(path) {
        let _ = file.write_all(&tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::test_support::temp_test_dir;

    fn write_file(dir: &Path, name: &str, content: &[u8]) {
        fs::write(dir.join(name), content).expect("failed to write test file");
    }

    #[test]
    fn daily_log_date_only_matches_daily_pattern() {
        assert!(daily_log_date("20260821.log").is_some());
        assert!(daily_log_date("panic.log").is_none());
        assert!(daily_log_date("startup-error.log").is_none());
        assert!(daily_log_date("20261340.log").is_none());
        assert!(daily_log_date("20260821.log.bak").is_none());
    }

    #[test]
    fn prune_removes_files_older_than_retention() {
        let dir = temp_test_dir("retention-days");
        let today = Local::now().date_naive();
        let old = (today - Duration::days(DAILY_LOG_RETENTION_DAYS + 1))
            .format("%Y%m%d")
            .to_string();
        let kept = today.format("%Y%m%d").to_string();
        write_file(&dir, &format!("{old}.log"), b"old");
        write_file(&dir, &format!("{kept}.log"), b"new");
        write_file(&dir, "panic.log", b"panic");

        let removed = prune_daily_logs(&dir).expect("prune failed");
        assert_eq!(removed, 1);
        assert!(!dir.join(format!("{old}.log")).exists());
        assert!(dir.join(format!("{kept}.log")).exists());
        assert!(dir.join("panic.log").exists());
    }

    #[test]
    fn prune_enforces_max_file_count() {
        let dir = temp_test_dir("retention-count");
        let today = Local::now().date_naive();
        let total = DAILY_LOG_MAX_FILES + 3;
        for offset in 0..total {
            let date = (today - Duration::days(offset as i64))
                .format("%Y%m%d")
                .to_string();
            write_file(&dir, &format!("{date}.log"), b"x");
        }

        let removed = prune_daily_logs(&dir).expect("prune failed");
        // Offsets 0..=7 survive the date rule; the count rule only kicks in
        // below that, so the 8 most recent files remain.
        let expected_kept = (DAILY_LOG_RETENTION_DAYS as usize + 1).min(DAILY_LOG_MAX_FILES);
        assert_eq!(removed, total - expected_kept);
        assert_eq!(daily_log_files(&dir).unwrap().len(), expected_kept);
        let oldest_kept = (today - Duration::days(DAILY_LOG_RETENTION_DAYS))
            .format("%Y%m%d")
            .to_string();
        assert!(dir.join(format!("{oldest_kept}.log")).exists());
    }

    #[test]
    fn cap_emergency_file_keeps_tail() {
        let dir = temp_test_dir("emergency-cap");
        let path = dir.join("panic.log");
        let mut content = b"first line\n".to_vec();
        for index in 0..100 {
            content.extend_from_slice(format!("line {index}\n").as_bytes());
        }
        fs::write(&path, &content).unwrap();

        cap_emergency_file(&path, 256);
        let capped = fs::read_to_string(&path).unwrap();
        assert!(capped.len() <= 128);
        assert!(capped.contains("line 99"));
        assert!(!capped.contains("first line"));

        // Small files are untouched.
        let before = fs::read(&path).unwrap();
        cap_emergency_file(&path, 1024);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
