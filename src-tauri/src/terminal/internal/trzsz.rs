use std::{collections::HashMap, io::SeekFrom, path::PathBuf, time::UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::{
    logging,
    state::AppState,
    terminal::internal::{
        core::{TrzszChooseDownloadDirectoryRequest, TrzszChooseUploadFilesRequest},
        sftp::expand_local_path,
        sftp_dialogs::{choose_trzsz_save_directory, choose_trzsz_upload_files},
        util::{normalize_terminal_transfer_name, unique_terminal_transfer_download_path},
    },
};

const TRZSZ_SCOPE: &str = "terminal.trzsz";
const TRZSZ_CHUNK_MAX_BYTES: usize = 1024 * 1024;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrzszChecksumChunkRequest {
    pub(crate) checksum_id: String,
    pub(crate) data_base64: String,
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

#[derive(Clone)]
pub(crate) struct TrzszDownloadSession {
    file_entry: TrzszEntry,
    bytes_written: u64,
    checksum: TrzszChecksum,
}

#[derive(Clone)]
pub(crate) struct TrzszChecksum {
    context: md5::Context,
    bytes_hashed: u64,
}

impl Default for TrzszChecksum {
    fn default() -> Self {
        Self {
            context: md5::Context::new(),
            bytes_hashed: 0,
        }
    }
}

#[derive(Default)]
pub(crate) struct TrzszRuntime {
    pub(crate) entries: HashMap<String, TrzszEntry>,
    pub(crate) downloads: HashMap<String, TrzszDownloadSession>,
    pub(crate) upload_checksums: HashMap<String, TrzszChecksum>,
    pub(crate) checksums: HashMap<String, TrzszChecksum>,
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
    let descriptor = register_directory(state.inner(), path).await?;
    logging::event(TRZSZ_SCOPE, "picker.download_directory.selected")
        .field("entry_id", &descriptor.entry_id)
        .field("name", &descriptor.name)
        .debug();
    Ok(Some(descriptor))
}

#[tauri::command]
pub(crate) async fn trzsz_get_entry(
    state: tauri::State<'_, AppState>,
    request: TrzszEntryRequest,
) -> Result<TrzszEntryDescriptor, String> {
    let entry = {
        let runtime = lock_runtime(state.inner());
        runtime
            .entries
            .get(&request.entry_id)
            .cloned()
            .ok_or_else(|| format!("trzsz entry '{}' was not found", request.entry_id))?
    };
    build_descriptor_async(&entry).await
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
    let length = request
        .length
        .clamp(1, TRZSZ_CHUNK_MAX_BYTES.min(TRZSZ_CHUNK_MAX_BYTES));
    let mut file = tokio::fs::File::open(&entry.path)
        .await
        .map_err(|e| format!("failed to open upload file '{}': {e}", entry.path.display()))?;
    file.seek(SeekFrom::Start(request.offset))
        .await
        .map_err(|e| format!("failed to seek upload file '{}': {e}", entry.path.display()))?;
    let mut buffer = vec![0_u8; length];
    let bytes_read = file
        .read(&mut buffer)
        .await
        .map_err(|e| format!("failed to read upload file '{}': {e}", entry.path.display()))?;
    buffer.truncate(bytes_read);

    {
        let mut runtime = lock_runtime(state.inner());
        let checksum = runtime
            .upload_checksums
            .entry(request.entry_id.clone())
            .or_default();
        if request.offset == 0 && checksum.bytes_hashed != 0 {
            *checksum = TrzszChecksum::default();
        }
        if request.offset != checksum.bytes_hashed {
            return Err(format!(
                "trzsz upload checksum for '{}' is non-sequential: expected offset {}, got {}",
                request.entry_id, checksum.bytes_hashed, request.offset
            ));
        }
        checksum.context.consume(&buffer);
        checksum.bytes_hashed = checksum.bytes_hashed.saturating_add(bytes_read as u64);
    }

    Ok(TrzszReadFileChunkResult {
        data_base64: STANDARD_NO_PAD.encode(&buffer),
        bytes_read,
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
    let path = unique_terminal_transfer_download_path(&parent.path, &file_name);
    tokio::fs::File::create(&path)
        .await
        .map_err(|e| format!("failed to create download file '{}': {e}", path.display()))?;
    let (file_entry, metadata) = register_entry_with_metadata_async(state.inner(), path).await?;
    let transfer_id = crate::ids::new_id();
    let descriptor = file_entry.descriptor(metadata);

    {
        let mut runtime = lock_runtime(state.inner());
        runtime.downloads.insert(
            transfer_id.clone(),
            TrzszDownloadSession {
                file_entry,
                bytes_written: 0,
                checksum: TrzszChecksum::default(),
            },
        );
    }

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

    let path = {
        let runtime = lock_runtime(state.inner());
        runtime
            .downloads
            .get(&request.transfer_id)
            .map(|download| download.file_entry.path.clone())
            .ok_or_else(|| format!("trzsz download '{}' is not active", request.transfer_id))?
    };

    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .await
        .map_err(|e| format!("failed to open download file '{}': {e}", path.display()))?;
    file.write_all(&bytes)
        .await
        .map_err(|e| format!("failed to write download file '{}': {e}", path.display()))?;

    {
        let mut runtime = lock_runtime(state.inner());
        let download = runtime
            .downloads
            .get_mut(&request.transfer_id)
            .ok_or_else(|| format!("trzsz download '{}' is not active", request.transfer_id))?;
        download.checksum.context.consume(&bytes);
        download.checksum.bytes_hashed = download
            .checksum
            .bytes_hashed
            .saturating_add(bytes.len() as u64);
        download.bytes_written = download.bytes_written.saturating_add(bytes.len() as u64);
    }

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

    let descriptor = build_descriptor_async(&download.file_entry).await?;
    if request.aborted {
        logging::event(TRZSZ_SCOPE, "download.aborted")
            .field("transfer_id", &request.transfer_id)
            .field("name", &descriptor.name)
            .warn();
    } else {
        logging::event(TRZSZ_SCOPE, "download.finished")
            .field("transfer_id", &request.transfer_id)
            .field("name", &descriptor.name)
            .field("bytes", download.bytes_written)
            .info();
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn trzsz_finish_upload_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszEntryRequest,
) -> Result<TrzszChecksumResult, String> {
    let checksum = {
        let mut runtime = lock_runtime(state.inner());
        runtime
            .upload_checksums
            .remove(&request.entry_id)
            .unwrap_or_default()
    };
    Ok(TrzszChecksumResult {
        checksum_id: request.entry_id,
        digest_base64: checksum_digest_base64(checksum),
    })
}

#[tauri::command]
pub(crate) fn trzsz_get_download_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszChecksumRequest,
) -> Result<TrzszChecksumResult, String> {
    let checksum = {
        let runtime = lock_runtime(state.inner());
        runtime
            .downloads
            .get(&request.checksum_id)
            .map(|download| download.checksum.clone())
            .ok_or_else(|| format!("trzsz download '{}' is not active", request.checksum_id))?
    };
    Ok(TrzszChecksumResult {
        checksum_id: request.checksum_id,
        digest_base64: checksum_digest_base64(checksum),
    })
}

#[tauri::command]
pub(crate) fn trzsz_begin_checksum(
    state: tauri::State<'_, AppState>,
) -> Result<TrzszChecksumResult, String> {
    let checksum_id = crate::ids::new_id();
    {
        let mut runtime = lock_runtime(state.inner());
        runtime
            .checksums
            .insert(checksum_id.clone(), TrzszChecksum::default());
    }
    Ok(TrzszChecksumResult {
        checksum_id,
        digest_base64: checksum_digest_base64(TrzszChecksum::default()),
    })
}

#[tauri::command]
pub(crate) fn trzsz_update_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszChecksumChunkRequest,
) -> Result<(), String> {
    let bytes = STANDARD_NO_PAD
        .decode(request.data_base64.as_bytes())
        .map_err(|e| format!("invalid trzsz checksum chunk base64: {e}"))?;
    let mut runtime = lock_runtime(state.inner());
    let checksum = runtime
        .checksums
        .get_mut(&request.checksum_id)
        .ok_or_else(|| format!("trzsz checksum '{}' is not active", request.checksum_id))?;
    checksum.context.consume(&bytes);
    checksum.bytes_hashed = checksum.bytes_hashed.saturating_add(bytes.len() as u64);
    Ok(())
}

#[tauri::command]
pub(crate) fn trzsz_finish_checksum(
    state: tauri::State<'_, AppState>,
    request: TrzszChecksumRequest,
) -> Result<TrzszChecksumResult, String> {
    let checksum = {
        let mut runtime = lock_runtime(state.inner());
        runtime
            .checksums
            .remove(&request.checksum_id)
            .ok_or_else(|| format!("trzsz checksum '{}' is not active", request.checksum_id))?
    };
    Ok(TrzszChecksumResult {
        checksum_id: request.checksum_id,
        digest_base64: checksum_digest_base64(checksum),
    })
}

fn lock_runtime(state: &crate::state::AppState) -> parking_lot::MutexGuard<'_, TrzszRuntime> {
    state.trzsz_runtime()
}

fn checksum_digest_base64(checksum: TrzszChecksum) -> String {
    STANDARD_NO_PAD.encode(checksum.context.finalize().0)
}

async fn register_paths(
    state: &AppState,
    paths: Vec<String>,
) -> Result<Vec<TrzszEntryDescriptor>, String> {
    let mut descriptors = Vec::new();
    for path in paths {
        let path = expand_local_path(&path)?;
        let (entry, metadata) = register_entry_with_metadata_async(state, path).await?;
        descriptors.push(entry.descriptor(metadata));
    }
    Ok(descriptors)
}

async fn register_directory(
    state: &AppState,
    path: String,
) -> Result<TrzszEntryDescriptor, String> {
    let path = expand_local_path(&path)?;
    let (entry, metadata) = register_entry_with_metadata_async(state, path).await?;
    Ok(entry.descriptor(metadata))
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

async fn build_descriptor_async(entry: &TrzszEntry) -> Result<TrzszEntryDescriptor, String> {
    let metadata = tokio::fs::metadata(&entry.path).await.map_err(|e| {
        format!(
            "failed to read entry metadata '{}': {e}",
            entry.path.display()
        )
    })?;
    Ok(entry.descriptor(metadata))
}
