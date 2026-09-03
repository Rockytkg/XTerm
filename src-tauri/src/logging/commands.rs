use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::{
    level::{log_level_name, parse_log_level, persisted_log_level, set_active_level},
    retention::prune_daily_logs,
};
use crate::{state::AppState, storage::SettingsRepository};

const DEFAULT_TAIL_BYTES: u64 = 64 * 1024;
const MAX_TAIL_BYTES: u64 = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    name: String,
    size_bytes: u64,
    /// Local timestamp `YYYY-MM-DD HH:MM:SS`; `None` when mtime is unreadable.
    modified_at: Option<String>,
}

fn log_dir(state: &AppState) -> PathBuf {
    state.paths().log_dir().to_path_buf()
}

/// Resolves a user-supplied file name to a path inside `dir`, rejecting
/// anything that is not a plain `*.log` file name (path traversal guard for
/// `log_file_tail`).
fn validated_log_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let valid = !name.is_empty()
        && name.ends_with(".log")
        && !name.contains(['/', '\\'])
        && !name.contains("..");
    if !valid {
        return Err(format!("invalid log file name '{name}'"));
    }
    let path = dir.join(name);
    if !path.is_file() {
        return Err(format!("log file '{name}' does not exist"));
    }
    Ok(path)
}

fn read_tail(path: &Path, max_bytes: u64) -> Result<String, String> {
    let buffer = super::retention::read_file_tail(path, max_bytes)
        .map_err(|error| format!("failed to read log file: {error}"))?;
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

#[tauri::command]
pub fn log_level_get(state: State<'_, AppState>) -> Result<String, String> {
    let store = state.store();
    Ok(log_level_name(persisted_log_level(&store)).to_string())
}

#[tauri::command]
pub fn log_level_set(state: State<'_, AppState>, level: String) -> Result<String, String> {
    let parsed = parse_log_level(&level)?;
    let normalized = log_level_name(parsed).to_string();
    // Update both the call-site gate (`log::set_max_level`, consulted by the
    // `log` macros) and the shared runtime level (consulted by the
    // daily-file dispatch's per-crate clamp), then persist.
    log::set_max_level(parsed);
    set_active_level(parsed);
    let store = state.store();
    SettingsRepository::set_log_level(&*store, &normalized)?;
    super::event("logging.level", "log-level.update")
        .field("level", &normalized)
        .info();
    Ok(normalized)
}

#[tauri::command]
pub fn log_files_list(state: State<'_, AppState>) -> Result<Vec<LogFileInfo>, String> {
    let dir = log_dir(&state);
    let entries =
        fs::read_dir(&dir).map_err(|error| format!("failed to read log directory: {error}"))?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".log") {
            continue;
        }
        let metadata = entry.metadata().ok();
        let size_bytes = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
        let modified_at = metadata.and_then(|meta| meta.modified().ok()).map(|time| {
            let datetime: chrono::DateTime<chrono::Local> = time.into();
            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
        });
        files.push(LogFileInfo {
            name,
            size_bytes,
            modified_at,
        });
    }
    files.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(files)
}

#[tauri::command]
pub fn log_file_tail(
    state: State<'_, AppState>,
    name: String,
    max_bytes: Option<u64>,
) -> Result<String, String> {
    let dir = log_dir(&state);
    let path = validated_log_path(&dir, &name)?;
    let max_bytes = max_bytes
        .unwrap_or(DEFAULT_TAIL_BYTES)
        .clamp(1024, MAX_TAIL_BYTES);
    read_tail(&path, max_bytes)
}

#[tauri::command]
pub fn log_files_prune(state: State<'_, AppState>) -> Result<usize, String> {
    let dir = log_dir(&state);
    let removed = prune_daily_logs(&dir)?;
    super::event("logging.retention", "log-files.prune")
        .field("removed", removed)
        .info();
    Ok(removed)
}

#[tauri::command]
pub fn log_dir_open(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = log_dir(&state);
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| format!("failed to open log directory: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_log_path_rejects_traversal() {
        let dir = Path::new("logs");
        for name in ["../x.log", "..\\x.log", "a/b.log", "x.txt", "", ".log/.."] {
            assert!(validated_log_path(dir, name).is_err(), "accepted {name}");
        }
    }

    #[test]
    fn read_tail_skips_partial_first_line() {
        let dir = crate::logging::test_support::temp_test_dir("tail");
        let path = dir.join("20260821.log");
        fs::write(&path, b"line one\nline two\nline three\n").unwrap();

        let full = read_tail(&path, 1024).unwrap();
        assert_eq!(full, "line one\nline two\nline three\n");

        let tail = read_tail(&path, 20).unwrap();
        assert!(tail.starts_with("line two") || tail.starts_with("line three"));
        assert!(!tail.contains("one"));

        fs::remove_dir_all(&dir).ok();
    }
}
