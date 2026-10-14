use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{
    logging,
    paths::expand_home_tilde,
    state::AppState,
    terminal::internal::{
        core::{TrzszChooseDownloadDirectoryRequest, TrzszChooseUploadFilesRequest},
        sftp_dialogs::{choose_trzsz_save_directory, choose_trzsz_upload_files},
        util::{normalize_terminal_transfer_name, unique_terminal_transfer_download_path},
    },
};

const TRZSZ_SCOPE: &str = "terminal.trzsz";
// 前端自适应分块上限默认 10 MiB，这里留到 16 MiB 余量；base64 后单条 IPC 约 21 MiB 可接受
const TRZSZ_CHUNK_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszFileChooserRequest {
    #[serde(default)]
    pub(crate) directory: bool,
    pub(crate) title: Option<String>,
    pub(crate) all_files_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszDirectoryChooserRequest {
    pub(crate) title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszEntryDescriptor {
    pub(crate) entry_id: String,
    pub(crate) kind: String,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) modified: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszEntryRequest {
    pub(crate) entry_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszReadFileChunkRequest {
    pub(crate) entry_id: String,
    pub(crate) offset: u64,
    pub(crate) length: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszReadFileChunkResult {
    pub(crate) data_base64: String,
    pub(crate) bytes_read: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszEnsureDirectoryRequest {
    pub(crate) directory_id: String,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszBeginDownloadRequest {
    pub(crate) directory_id: String,
    pub(crate) file_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszBeginDownloadResult {
    pub(crate) transfer_id: String,
    pub(crate) entry: TrzszEntryDescriptor,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszWriteDownloadChunkRequest {
    pub(crate) transfer_id: String,
    pub(crate) data_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszFinishDownloadRequest {
    pub(crate) transfer_id: String,
    #[serde(default)]
    pub(crate) aborted: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszChecksumRequest {
    pub(crate) checksum_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszChecksumResult {
    pub(crate) checksum_id: String,
    pub(crate) digest_base64: String,
}

#[derive(Clone, Debug)]
pub(crate) enum TrzszEntryKind {
    File,
    Directory,
}

impl TrzszEntryKind {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        if metadata.is_dir() {
            Self::Directory
        } else {
            Self::File
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrzszEntry {
    entry_id: String,
    path: PathBuf,
    kind: TrzszEntryKind,
}

impl TrzszEntry {
    fn new(path: PathBuf, kind: TrzszEntryKind) -> Self {
        Self {
            entry_id: crate::ids::new_id(),
            path,
            kind,
        }
    }

    fn from_metadata(path: PathBuf, metadata: &std::fs::Metadata) -> Self {
        Self::new(path, TrzszEntryKind::from_metadata(metadata))
    }

    fn descriptor(&self, metadata: std::fs::Metadata) -> TrzszEntryDescriptor {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs());
        TrzszEntryDescriptor {
            entry_id: self.entry_id.clone(),
            kind: self.kind.as_str().to_string(),
            name: self.display_name(),
            size: metadata.len(),
            modified,
        }
    }

    fn display_name(&self) -> String {
        let fallback = if matches!(self.kind, TrzszEntryKind::Directory) {
            "folder"
        } else {
            "file"
        };
        self.path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(fallback)
            .to_string()
    }
}

/// 一次传输内保持打开的文件句柄：offset 同时充当顺序性校验和已传输字节数，
/// checksum 随每次读写增量更新，避免按块反复 open/seek/close。
pub(crate) struct TrzszFileSession {
    file: std::fs::File,
    offset: u64,
    checksum: md5::Context,
}

impl TrzszFileSession {
    fn new(file: std::fs::File) -> Self {
        Self {
            file,
            offset: 0,
            checksum: md5::Context::new(),
        }
    }

    fn digest_base64(&self) -> String {
        STANDARD_NO_PAD.encode(self.checksum.clone().finalize().0)
    }
}

type SharedFileSession = Arc<Mutex<TrzszFileSession>>;

pub(crate) struct TrzszDownloadSession {
    session: SharedFileSession,
    /// 最终落盘路径（begin 时按最终名做唯一名决策，finish 非中止时才出现）。
    path: PathBuf,
    /// 下载全程实际写入的 `<最终名>.part`，断线/会话关闭留下的部分文件
    /// 因此不会冒充完整文件。
    part_path: PathBuf,
    name: String,
}

#[derive(Default)]
pub(crate) struct TrzszRuntime {
    pub(crate) entries: HashMap<String, TrzszEntry>,
    uploads: HashMap<String, SharedFileSession>,
    downloads: HashMap<String, TrzszDownloadSession>,
}

impl TrzszRuntime {
    fn register_entry(&mut self, entry: TrzszEntry) -> TrzszEntry {
        self.entries.insert(entry.entry_id.clone(), entry.clone());
        entry
    }
}

#[tauri::command]
pub(crate) async fn trzsz_register_drag_paths(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<TrzszEntryDescriptor>, String> {
    register_paths(state.inner(), paths).await
}

#[tauri::command]
pub(crate) async fn trzsz_choose_upload_entries(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    request: TrzszFileChooserRequest,
) -> Result<Vec<TrzszEntryDescriptor>, String> {
    logging::event(TRZSZ_SCOPE, "picker.upload.start")
        .field("directory", request.directory)
        .maybe_field("title", request.title.clone())
        .debug();
    let dialog_request = TrzszChooseUploadFilesRequest {
        directory: request.directory,
        title: request.title.clone(),
        all_files_label: request.all_files_label.clone(),
    };
    let paths = choose_trzsz_upload_files(&app, &dialog_request).await?;
    if paths.is_empty() {
        logging::event(TRZSZ_SCOPE, "picker.upload.cancelled")
            .field("directory", request.directory)
            .debug();
        return Ok(Vec::new());
    }
    let descriptors = register_paths(state.inner(), paths).await?;
    logging::event(TRZSZ_SCOPE, "picker.upload.selected")
        .field("directory", request.directory)
        .field("count", descriptors.len())
        .debug();
    Ok(descriptors)
}

#[tauri::command]
pub(crate) async fn trzsz_choose_download_directory(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    request: TrzszDirectoryChooserRequest,
) -> Result<Option<TrzszEntryDescriptor>, String> {
    logging::event(TRZSZ_SCOPE, "picker.download_directory.start")
        .maybe_field("title", request.title.clone())
        .debug();
    let dialog_request = TrzszChooseDownloadDirectoryRequest {
        title: request.title.clone(),
    };
    let Some(path) = choose_trzsz_save_directory(&app, &dialog_request).await? else {
        logging::event(TRZSZ_SCOPE, "picker.download_directory.cancelled").debug();
        return Ok(None);
    };
    let descriptor = register_path(state.inner(), &path).await?;
    logging::event(TRZSZ_SCOPE, "picker.download_directory.selected")
        .field("entry_id", &descriptor.entry_id)
        .field("name", &descriptor.name)
        .debug();
    Ok(Some(descriptor))
}

#[tauri::command]
pub(crate) async fn trzsz_list_directory(
    state: tauri::State<'_, AppState>,
    request: TrzszEntryRequest,
) -> Result<Vec<TrzszEntryDescriptor>, String> {
    let entry = get_registered_entry(state.inner(), &request.entry_id)?;
    if !matches!(entry.kind, TrzszEntryKind::Directory) {
        return Err(format!(
            "trzsz entry '{}' is not a directory",
            request.entry_id
        ));
    }
    let metadata = tokio::fs::metadata(&entry.path).await.map_err(|e| {
        format!(
            "failed to read directory metadata '{}': {e}",
            entry.path.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "directory '{}' does not exist",
            entry.path.display()
        ));
    }

    let mut child_entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&entry.path)
        .await
        .map_err(|e| format!("failed to list directory '{}': {e}", entry.path.display()))?;
    while let Some(dir_entry) = dir.next_entry().await.map_err(|e| {
        format!(
            "failed to read directory entry '{}': {e}",
            entry.path.display()
        )
    })? {
        let child_path = dir_entry.path();
        let (child_entry, metadata) =
            register_entry_with_metadata_async(state.inner(), child_path).await?;
        child_entries.push(child_entry.descriptor(metadata));
    }
    child_entries.sort_by(|a, b| {
        let a_dir = a.kind == "directory";
        let b_dir = b.kind == "directory";
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(child_entries)
}

#[tauri::command]
pub(crate) async fn trzsz_read_file_chunk(
    state: tauri::State<'_, AppState>,
    request: TrzszReadFileChunkRequest,
) -> Result<TrzszReadFileChunkResult, String> {
    let entry = get_registered_entry(state.inner(), &request.entry_id)?;
    if !matches!(entry.kind, TrzszEntryKind::File) {
        return Err(format!("trzsz entry '{}' is not a file", request.entry_id));
    }
    let length = request.length.clamp(1, TRZSZ_CHUNK_MAX_BYTES);

    let session = if request.offset == 0 {
        // 每次从 0 开始视为新上传，重置该 entry 的会话（覆盖中止后重传的场景）
        let file = std::fs::File::open(&entry.path)
            .map_err(|e| format!("failed to open upload file '{}': {e}", entry.path.display()))?;
        let session: SharedFileSession = Arc::new(Mutex::new(TrzszFileSession::new(file)));
        lock_runtime(state.inner())
            .uploads
            .insert(request.entry_id.clone(), session.clone());
        session
    } else {
        lock_runtime(state.inner())
            .uploads
            .get(&request.entry_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "trzsz upload '{}' did not start at offset 0",
                    request.entry_id
                )
            })?
    };

    let entry_id = request.entry_id.clone();
    let expected_offset = request.offset;
    let buffer = tokio::task::spawn_blocking(move || {
        let mut session = session
            .lock()
            .map_err(|_| format!("trzsz upload session '{entry_id}' is poisoned"))?;
        if session.offset != expected_offset {
            return Err(format!(
                "trzsz upload checksum for '{entry_id}' is non-sequential: expected offset {expected_offset}, got {}",
                session.offset
            ));
        }
        let mut buffer = vec![0_u8; length];
        let bytes_read = session
            .file
            .read(&mut buffer)
            .map_err(|e| format!("failed to read upload file: {e}"))?;
        buffer.truncate(bytes_read);
        session.checksum.consume(&buffer);
        session.offset += bytes_read as u64;
        Ok::<Vec<u8>, String>(buffer)
    })
    .await
    .map_err(|e| format!("trzsz upload read task failed: {e}"))??;

    Ok(TrzszReadFileChunkResult {
        bytes_read: buffer.len(),
        data_base64: STANDARD_NO_PAD.encode(&buffer),
    })
}

#[tauri::command]
pub(crate) async fn trzsz_ensure_directory(
    state: tauri::State<'_, AppState>,
    request: TrzszEnsureDirectoryRequest,
) -> Result<TrzszEntryDescriptor, String> {
    let parent = get_registered_entry(state.inner(), &request.directory_id)?;
    if !matches!(parent.kind, TrzszEntryKind::Directory) {
        return Err(format!(
            "trzsz directory '{}' is not writable",
            request.directory_id
        ));
    }
    let name = normalize_terminal_transfer_name(&request.name, "folder");
    let path = parent.path.join(&name);
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("failed to create directory '{}': {e}", path.display()))?;
    let (entry, metadata) = register_entry_with_metadata_async(state.inner(), path).await?;
    Ok(entry.descriptor(metadata))
}

#[tauri::command]
pub(crate) async fn trzsz_begin_download(
    state: tauri::State<'_, AppState>,
    request: TrzszBeginDownloadRequest,
) -> Result<TrzszBeginDownloadResult, String> {
    let parent = get_registered_entry(state.inner(), &request.directory_id)?;
    if !matches!(parent.kind, TrzszEntryKind::Directory) {
        return Err(format!(
            "trzsz directory '{}' is not writable",
            request.directory_id
        ));
    }
    let file_name = normalize_terminal_transfer_name(&request.file_name, "download");
    // 冲突判定在最终名上做（沿用唯一名决策，永不覆盖既有文件）；
    // 实际写入 .part，finish 非中止时才重命名为最终名。
    let path = unique_terminal_transfer_download_path(&parent.path, &file_name);
    let part_path = download_part_path(&path);
    let file = std::fs::File::create(&part_path).map_err(|e| {
        format!(
            "failed to create download file '{}': {e}",
            part_path.display()
        )
    })?;
    // 最终路径此刻不存在，元数据取自刚创建的 .part；注册条目仍指向最终路径，
    // 使下发名称与成功 finish 后的落盘文件一致。
    let metadata = tokio::fs::metadata(&part_path).await.map_err(|e| {
        format!(
            "failed to read entry metadata '{}': {e}",
            part_path.display()
        )
    })?;
    let transfer_id = crate::ids::new_id();
    let descriptor = {
        let mut runtime = lock_runtime(state.inner());
        let file_entry = runtime.register_entry(TrzszEntry::new(path, TrzszEntryKind::File));
        let descriptor = file_entry.descriptor(metadata);
        runtime.downloads.insert(
            transfer_id.clone(),
            TrzszDownloadSession {
                session: Arc::new(Mutex::new(TrzszFileSession::new(file))),
                path: file_entry.path.clone(),
                part_path,
                name: descriptor.name.clone(),
            },
        );
        descriptor
    };

    logging::event(TRZSZ_SCOPE, "download.begin")
        .field("transfer_id", &transfer_id)
        .field("name", &descriptor.name)
        .info();

    Ok(TrzszBeginDownloadResult {
        transfer_id,
        entry: descriptor,
    })
}

#[tauri::command]
pub(crate) async fn trzsz_write_download_chunk(
    state: tauri::State<'_, AppState>,
    request: TrzszWriteDownloadChunkRequest,
) -> Result<(), String> {
    let bytes = STANDARD_NO_PAD
        .decode(request.data_base64.as_bytes())
        .map_err(|e| format!("invalid trzsz download chunk base64: {e}"))?;

    let session = lock_runtime(state.inner())
        .downloads
        .get(&request.transfer_id)
        .map(|download| download.session.clone())
        .ok_or_else(|| format!("trzsz download '{}' is not active", request.transfer_id))?;

    tokio::task::spawn_blocking(move || {
        let mut session = session
            .lock()
            .map_err(|_| "trzsz download session is poisoned".to_string())?;
        session
            .file
            .write_all(&bytes)
            .map_err(|e| format!("failed to write download file: {e}"))?;
        session.checksum.consume(&bytes);
        session.offset += bytes.len() as u64;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("trzsz download write task failed: {e}"))??;

    Ok(())
}

#[tauri::command]
pub(crate) async fn trzsz_finish_download(
    state: tauri::State<'_, AppState>,
    request: TrzszFinishDownloadRequest,
) -> Result<(), String> {
    let download = {
        let mut runtime = lock_runtime(state.inner());
        runtime.downloads.remove(&request.transfer_id)
    };

    let Some(download) = download else {
        return Ok(());
    };
    let part_path = download.part_path.clone();

    let bytes_written = tokio::task::spawn_blocking(move || {
        let mut session = download
            .session
            .lock()
            .map_err(|_| "trzsz download session is poisoned".to_string())?;
        session.file.flush().map_err(|e| {
            format!(
                "failed to flush download file '{}': {e}",
                part_path.display()
            )
        })?;
        Ok::<u64, String>(session.offset)
    })
    .await
    .map_err(|e| format!("trzsz download finish task failed: {e}"))??;

    if request.aborted {
        // 中止语义只来自前端 deleteFile（如 MD5 校验失败），必须把已落盘的损坏
        // .part 一并删除（最终名从头到尾不出现）；删除失败仅记录，不影响会话清理
        if let Err(error) = discard_download_part(&download.part_path) {
            logging::event(TRZSZ_SCOPE, "download.abort_remove_failed")
                .field("transfer_id", &request.transfer_id)
                .field("path", download.part_path.display().to_string())
                .field("error", error.to_string())
                .warn();
        }
        logging::event(TRZSZ_SCOPE, "download.aborted")
            .field("transfer_id", &request.transfer_id)
            .field("name", &download.name)
            .warn();
    } else {
        finalize_download_part(&download.part_path, &download.path)?;
        logging::event(TRZSZ_SCOPE, "download.finished")
            .field("transfer_id", &request.transfer_id)
            .field("name", &download.name)
            .field("bytes", bytes_written)
            .info();
    }
    Ok(())
}

/// 下载全程写入的实际路径：`<最终名>.part`。断线/会话关闭导致 finish 缺失时，
/// 残留的部分文件一眼可辨，不会冒充完整文件；同名重传时 begin 的唯一名决策在
/// 最终名上做，陈旧的 .part 会被新传输直接截断复用。
fn download_part_path(final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .map(|name| format!("{}.part", name.to_string_lossy()))
        .unwrap_or_else(|| "download.part".to_string());
    final_path.with_file_name(file_name)
}

/// 把 `.part` 落定为最终名。begin 的唯一名决策基于当时的目录状态，下载期间最终名
/// 可能被其它进程占用，而 rename 在 Windows 上不覆盖既有文件（Unix 则会静默覆盖）
/// ——两边都不允许覆盖：被占用时重新取唯一名，保持 begin 的"永不覆盖既有文件"语义
/// （参考 session_recording rotate_recording 对 rename 覆盖行为的处理）。
fn finalize_download_part(part_path: &Path, final_path: &Path) -> Result<PathBuf, String> {
    let target = if final_path.exists() {
        match (
            final_path.parent(),
            final_path.file_name().and_then(|name| name.to_str()),
        ) {
            (Some(parent), Some(name)) => unique_terminal_transfer_download_path(parent, name),
            _ => final_path.to_path_buf(),
        }
    } else {
        final_path.to_path_buf()
    };
    std::fs::rename(part_path, &target).map_err(|e| {
        format!(
            "failed to finalize download file '{}': {e}",
            target.display()
        )
    })?;
    Ok(target)
}

/// 中止下载：删除 `.part`，最终名从头到尾不出现。
fn discard_download_part(part_path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(part_path)
}

#[tauri::command]
pub(crate) fn trzsz_finish_upload_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszEntryRequest,
) -> Result<TrzszChecksumResult, String> {
    let session = {
        let mut runtime = lock_runtime(state.inner());
        runtime.uploads.remove(&request.entry_id)
    };
    let digest_base64 = match session {
        Some(session) => session
            .lock()
            .map_err(|_| format!("trzsz upload session '{}' is poisoned", request.entry_id))?
            .digest_base64(),
        None => STANDARD_NO_PAD.encode(md5::Context::new().finalize().0),
    };
    Ok(TrzszChecksumResult {
        checksum_id: request.entry_id,
        digest_base64,
    })
}

#[tauri::command]
pub(crate) fn trzsz_get_download_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszChecksumRequest,
) -> Result<TrzszChecksumResult, String> {
    let digest_base64 = {
        let session = {
            let runtime = lock_runtime(state.inner());
            runtime
                .downloads
                .get(&request.checksum_id)
                .map(|download| download.session.clone())
                .ok_or_else(|| format!("trzsz download '{}' is not active", request.checksum_id))?
        };
        let guard = session
            .lock()
            .map_err(|_| "trzsz download session is poisoned".to_string())?;
        guard.digest_base64()
    };
    Ok(TrzszChecksumResult {
        checksum_id: request.checksum_id,
        digest_base64,
    })
}

fn lock_runtime(state: &crate::state::AppState) -> parking_lot::MutexGuard<'_, TrzszRuntime> {
    state.trzsz_runtime()
}

async fn register_path(state: &AppState, path: &str) -> Result<TrzszEntryDescriptor, String> {
    let path = expand_home_tilde(path)?;
    let (entry, metadata) = register_entry_with_metadata_async(state, path).await?;
    Ok(entry.descriptor(metadata))
}

async fn register_paths(
    state: &AppState,
    paths: Vec<String>,
) -> Result<Vec<TrzszEntryDescriptor>, String> {
    let mut descriptors = Vec::with_capacity(paths.len());
    for path in &paths {
        descriptors.push(register_path(state, path).await?);
    }
    Ok(descriptors)
}

async fn register_entry_with_metadata_async(
    state: &AppState,
    path: PathBuf,
) -> Result<(TrzszEntry, std::fs::Metadata), String> {
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|e| format!("failed to read entry metadata '{}': {e}", path.display()))?;
    let entry = TrzszEntry::from_metadata(path, &metadata);
    let mut runtime = lock_runtime(state);
    Ok((runtime.register_entry(entry), metadata))
}

fn get_registered_entry(state: &AppState, entry_id: &str) -> Result<TrzszEntry, String> {
    let runtime = lock_runtime(state);
    runtime
        .entries
        .get(entry_id)
        .cloned()
        .ok_or_else(|| format!("trzsz entry '{entry_id}' was not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(content: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("xterm-trzsz-test-{}", crate::ids::new_id()));
        std::fs::write(&path, content).expect("write temp file");
        path
    }

    #[test]
    fn sequential_reads_track_offset_and_checksum() {
        let path = temp_file(b"hello trzsz");
        let file = std::fs::File::open(&path).expect("open temp file");
        let mut session = TrzszFileSession::new(file);

        let mut buffer = vec![0_u8; 5];
        let read = session.file.read(&mut buffer).expect("read first chunk");
        buffer.truncate(read);
        session.checksum.consume(&buffer);
        session.offset += read as u64;
        assert_eq!(session.offset, 5);
        assert_eq!(buffer, b"hello");

        assert_eq!(
            session.digest_base64(),
            STANDARD_NO_PAD.encode(md5::compute(b"hello").0)
        );

        std::fs::remove_file(&path).expect("remove temp file");
    }

    #[test]
    fn empty_session_digest_matches_empty_md5() {
        let path = temp_file(b"");
        let file = std::fs::File::open(&path).expect("open temp file");
        let session = TrzszFileSession::new(file);
        assert_eq!(
            session.digest_base64(),
            STANDARD_NO_PAD.encode(md5::compute(b"").0)
        );
        std::fs::remove_file(&path).expect("remove temp file");
    }

    #[test]
    fn part_path_appends_suffix_to_full_name() {
        let path = Path::new("downloads").join("archive.tar.gz");
        assert_eq!(
            download_part_path(&path),
            Path::new("downloads").join("archive.tar.gz.part")
        );
    }

    #[test]
    fn finalize_moves_part_to_final_name() {
        let dir = crate::logging::test_support::temp_test_dir("trzsz-finalize");
        let final_path = dir.join("report.txt");
        let part_path = dir.join("report.txt.part");
        std::fs::write(&part_path, b"payload").expect("write part file");

        let used = finalize_download_part(&part_path, &final_path).expect("finalize download");

        assert_eq!(used, final_path);
        assert_eq!(std::fs::read(&final_path).expect("read final"), b"payload");
        assert!(!part_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finalize_does_not_overwrite_file_that_appeared_mid_download() {
        let dir = crate::logging::test_support::temp_test_dir("trzsz-finalize-race");
        let final_path = dir.join("report.txt");
        let part_path = dir.join("report.txt.part");
        std::fs::write(&final_path, b"other").expect("write competing final file");
        std::fs::write(&part_path, b"payload").expect("write part file");

        let used = finalize_download_part(&part_path, &final_path).expect("finalize download");

        assert_eq!(used, dir.join("report (1).txt"));
        assert_eq!(std::fs::read(&final_path).expect("read final"), b"other");
        assert_eq!(std::fs::read(&used).expect("read renamed"), b"payload");
        assert!(!part_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discard_removes_part_and_final_never_appears() {
        let dir = crate::logging::test_support::temp_test_dir("trzsz-discard");
        let final_path = dir.join("report.txt");
        let part_path = dir.join("report.txt.part");
        std::fs::write(&part_path, b"partial").expect("write part file");

        discard_download_part(&part_path).expect("discard part file");

        assert!(!part_path.exists());
        assert!(!final_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
