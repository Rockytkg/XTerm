use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Deserialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// Maximum size of a single session recording file. When an append would
/// exceed this, the current file is rotated to `<name>.1` (replacing any
/// previous rotation) and recording continues in a fresh file.
const MAX_RECORDING_BYTES: u64 = 256 * 1024 * 1024;

/// Writers idle for longer than this are closed so a finished recording never
/// keeps its file locked.
const WRITER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn dialog_label(value: &Option<String>, fallback: &str) -> String {
    let label = value.as_deref().unwrap_or(fallback).trim();
    if label.is_empty() {
        fallback.to_string()
    } else {
        label.replace('|', " ")
    }
}

pub(crate) fn dialog_file_name(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let file_name = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    file_name
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecordingFileRequest {
    default_file_name: String,
    title: Option<String>,
    text_files_label: Option<String>,
    all_files_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecordingAppendRequest {
    path: String,
    data: String,
}

/// Opens a native save-file dialog and returns the transcript path selected by
/// the user. Recording is user-initiated, so the frontend provides only a
/// suggested file name; the final writable path always comes from the dialog.
#[tauri::command]
pub async fn session_recording_choose_file(
    app: AppHandle,
    request: SessionRecordingFileRequest,
) -> Result<Option<String>, String> {
    choose_recording_file(&app, &request).await
}

/// Appends one rendered terminal transcript chunk to the selected session log
/// file. The frontend reduces terminal control sequences before calling this
/// command so the file follows the text visible in xterm instead of raw PTY IO.
/// Parent directories must already exist because they come from the native save
/// dialog.
///
/// Files are written through a small pool of cached `File` handles: opening the
/// file on every 120 ms chunk was the previous bottleneck. Writes go straight
/// to the OS with no intermediate buffer — a `BufWriter` here previously
/// swallowed recordings smaller than its 8 KiB buffer because `static` state
/// never runs destructors, so nothing ever flushed them. The frontend already
/// batches output into 120 ms / 32 KiB chunks, so each append is a single
/// zero-copy `write_all` of the request bytes. A file that grows past
/// [`MAX_RECORDING_BYTES`] is rotated to `<name>.1` before the append.
#[tauri::command]
pub async fn session_recording_append(
    request: SessionRecordingAppendRequest,
) -> Result<(), String> {
    let path = PathBuf::from(request.path.trim());
    if path.as_os_str().is_empty() {
        return Err("recording path is empty".to_string());
    }

    append_recording(&path, request.data.as_bytes(), MAX_RECORDING_BYTES).map_err(|error| {
        let msg = format!("failed to append session recording: {error}");
        log::warn!(target: "session_recording", "{msg}");
        msg
    })
}

struct RecordingWriter {
    file: File,
    /// Size of the file as tracked by this writer (starts at the on-disk
    /// length when the file is opened in append mode).
    size: u64,
    last_used: Instant,
}

static RECORDING_WRITERS: parking_lot::Mutex<Option<HashMap<PathBuf, RecordingWriter>>> =
    parking_lot::Mutex::new(None);

fn append_recording(path: &Path, data: &[u8], max_bytes: u64) -> std::io::Result<()> {
    let mut writers = RECORDING_WRITERS.lock();
    let writers = writers.get_or_insert_with(HashMap::new);
    close_idle_writers(writers);

    if !writers.contains_key(path) {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata()?.len();
        writers.insert(
            path.to_path_buf(),
            RecordingWriter {
                file,
                size,
                last_used: Instant::now(),
            },
        );
    }
    let entry = writers
        .get_mut(path)
        .expect("recording writer was just inserted");
    entry.last_used = Instant::now();

    if entry.size.saturating_add(data.len() as u64) > max_bytes {
        rotate_recording(path, entry)?;
    }

    entry.file.write_all(data)?;
    entry.size = entry.size.saturating_add(data.len() as u64);
    Ok(())
}

/// Renames the current file to `<name>.1` (overwriting any older rotation) and
/// opens a fresh file so recording continues seamlessly. Writes go straight to
/// the OS, so there is nothing to flush before the rename; Rust opens files
/// with `FILE_SHARE_DELETE` on Windows, so renaming the still-open handle is
/// safe and avoids a close/open race with concurrent appends.
fn rotate_recording(path: &Path, entry: &mut RecordingWriter) -> std::io::Result<()> {
    let rotated = rotated_path(path);
    // Remove the previous rotation first: `rename` does not overwrite an
    // existing file on every platform (notably older Windows semantics).
    match std::fs::remove_file(&rotated) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::rename(path, &rotated)?;
    entry.file = OpenOptions::new().create(true).append(true).open(path)?;
    entry.size = 0;
    Ok(())
}

fn rotated_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| format!("{}.1", name.to_string_lossy()))
        .unwrap_or_else(|| "recording.1".to_string());
    path.with_file_name(file_name)
}

/// Writers idle for longer than [`WRITER_IDLE_TIMEOUT`] are closed (dropped)
/// so a finished recording never keeps its file locked. Writes are already on
/// disk by then because appends go straight to the file without buffering.
fn close_idle_writers(writers: &mut HashMap<PathBuf, RecordingWriter>) {
    writers.retain(|_path, entry| entry.last_used.elapsed() < WRITER_IDLE_TIMEOUT);
}

async fn choose_recording_file(
    app: &AppHandle,
    request: &SessionRecordingFileRequest,
) -> Result<Option<String>, String> {
    let safe_default = normalize_recording_file_name(&request.default_file_name);
    let title = dialog_label(&request.title, "Save session recording");
    let text_files_label = dialog_label(&request.text_files_label, "Text files");
    let all_files_label = dialog_label(&request.all_files_label, "All files");

    // Use Tauri's dialog plugin instead of a detached PowerShell/WinForms
    // process so the save panel belongs to this app and inherits native
    // window integration such as icon, modality, and OS localization.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(safe_default)
        .add_filter(text_files_label, &["txt"])
        .add_filter(all_files_label, &["*"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });

    receiver
        .await
        .map_err(|_| "session recording save dialog closed before returning a result".to_string())?
        .map(|path| {
            path.into_path()
                .map(|path| ensure_txt_extension(&path).to_string_lossy().to_string())
                .map_err(|error| format!("failed to resolve save path: {error}"))
        })
        .transpose()
}

fn normalize_recording_file_name(value: &str) -> String {
    let sanitized = dialog_file_name(value, "session-recording.txt");
    if sanitized.to_ascii_lowercase().ends_with(".txt") {
        sanitized
    } else {
        format!("{sanitized}.txt")
    }
}

fn ensure_txt_extension(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        path.with_extension("txt")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{append_recording, rotated_path};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique temporary directory per test so the shared writer pool never
    /// leaks state between cases.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "xterm-session-recording-test-{}-{}-{}",
            name,
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("failed to create test directory");
        dir
    }

    /// Regression: appends must be visible on disk immediately, without any
    /// flush or close. The old `BufWriter` pool kept chunks smaller than 8 KiB
    /// in memory forever (static state has no destructor), so recordings
    /// appeared as empty files.
    #[test]
    fn appended_content_is_on_disk_without_flush_or_close() {
        let dir = test_dir("immediate");
        let path = dir.join("session.txt");

        append_recording(&path, b"tiny chunk", 1024).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"tiny chunk");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotation_moves_previous_content_to_dot_1_and_continues_in_a_fresh_file() {
        let dir = test_dir("rotate");
        let path = dir.join("session.txt");
        let max = 10;

        append_recording(&path, b"aaaa", max).unwrap();
        append_recording(&path, b"bbbbbb", max).unwrap();
        // 4 + 6 == 10, not over the limit yet.
        assert!(!rotated_path(&path).exists());

        append_recording(&path, b"cc", max).unwrap();
        let rotated = fs::read(rotated_path(&path)).unwrap();
        assert_eq!(rotated, b"aaaabbbbbb");

        // The fresh file receives the new chunk.
        append_recording(&path, &[b'x'; 16 * 1024], u64::MAX).unwrap();
        let current = fs::read(&path).unwrap();
        assert!(current.starts_with(b"cc"));
        assert_eq!(current.len(), 2 + 16 * 1024);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn second_rotation_replaces_the_previous_dot_1_file() {
        let dir = test_dir("rotate-twice");
        let path = dir.join("session.txt");
        let max = 4;

        append_recording(&path, b"aaaa", max).unwrap();
        append_recording(&path, b"bbbb", max).unwrap();
        append_recording(&path, b"cccc", max).unwrap();

        assert_eq!(fs::read(rotated_path(&path)).unwrap(), b"bbbb");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn existing_file_size_counts_toward_the_rotation_limit() {
        let dir = test_dir("existing");
        let path = dir.join("session.txt");
        fs::write(&path, b"0123456789").unwrap();

        append_recording(&path, b"ab", 10).unwrap();

        assert_eq!(fs::read(rotated_path(&path)).unwrap(), b"0123456789");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_creates_missing_file() {
        let dir = test_dir("create");
        let path = dir.join("new.txt");

        append_recording(&path, b"hello", 1024).unwrap();

        assert!(path.exists());
        fs::remove_dir_all(&dir).ok();
    }
}
