use std::time::Duration;
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    logging,
    state::AppState,
    terminal::internal::{
        core::{
            SftpChooseDownloadPathRequest, SftpChooseUploadFilesRequest, SftpCloseSessionRequest,
            SftpCreateDirRequest, SftpCreateFileRequest, SftpDeleteRequest, SftpEntry,
            SftpFileStatResult, SftpListRemoteRequest, SftpListResult, SftpReadFileRequest,
            SftpRenameRequest, SftpStatFileRequest, SftpWriteFileRequest,
        },
        sftp::{
            delete_remote_path, ensure_remote_dir, join_remote_path, normalize_remote_path,
            remote_file_kind, remote_modified_timestamp, remote_parent_path, rename_remote_path,
            resolve_remote_child_path, sftp_file_stat_result, sort_sftp_entries,
            SftpNameConflictAction, SFTP_EDIT_MAX_BYTES,
        },
        sftp_dialogs::{choose_sftp_download_path, choose_sftp_upload_files},
        ssh_aux::get_or_create_sftp_session,
    },
};

const SFTP_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

#[tauri::command]
pub(crate) fn sftp_close_session(
    state: tauri::State<'_, AppState>,
    request: SftpCloseSessionRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.close_session")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .debug();
    if let Some(bound_connection_id) = state.connection_id_for_session(&request.session_id) {
        if bound_connection_id != request.connection_id {
            return Err(
                "The active terminal session does not match the requested connection.".to_string(),
            );
        }
    }
    state.remove_sftp_session(&request.session_id);
    Ok(())
}

#[tauri::command]
pub(crate) async fn sftp_list_remote(
    state: tauri::State<'_, AppState>,
    request: SftpListRemoteRequest,
) -> Result<SftpListResult, String> {
    logging::event("terminal.sftp", "sftp.list_remote.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &request.path)
        .debug();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = normalize_remote_path(&request.path);
    let read_path = path.clone();
    let entries = sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                sftp.read_dir(&read_path).await.map_err(|error| {
                    format!("failed to list remote directory '{read_path}': {error}")
                })
            })
        })
        .await?;
    let mut entries: Vec<SftpEntry> = entries
        .into_iter()
        .filter_map(|entry| {
            let name = entry.file_name().to_string();
            if name == "." || name == ".." {
                return None;
            }
            let attrs = entry.metadata();
            Some(SftpEntry {
                path: join_remote_path(&path, &name),
                name,
                kind: remote_file_kind(attrs.file_type()),
                size: attrs.len(),
                modified: remote_modified_timestamp(&attrs),
            })
        })
        .collect();
    sort_sftp_entries(&mut entries);
    logging::event("terminal.sftp", "sftp.list_remote.success")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &path)
        .field("entries", entries.len())
        .debug();
    Ok(SftpListResult {
        parent: remote_parent_path(&path),
        path,
        entries,
    })
}

#[tauri::command]
pub(crate) async fn sftp_delete(
    state: tauri::State<'_, AppState>,
    request: SftpDeleteRequest,
) -> Result<(), String> {
    if request.paths.is_empty() {
        return Err("no remote paths selected for deletion".to_string());
    }
    logging::event("terminal.sftp", "sftp.delete.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("count", request.paths.len())
        .warn();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                for path in request.paths {
                    delete_remote_path(sftp, &normalize_remote_path(&path)).await?;
                }
                Ok(())
            })
        })
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn sftp_create_dir(
    state: tauri::State<'_, AppState>,
    request: SftpCreateDirRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.create_dir")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("parent_path", &request.parent_path)
        .field("name", &request.name)
        .info();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = resolve_remote_child_path(&request.parent_path, &request.name)?;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move { ensure_remote_dir(sftp, &path).await })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_create_file(
    state: tauri::State<'_, AppState>,
    request: SftpCreateFileRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.create_file")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("parent_path", &request.parent_path)
        .field("name", &request.name)
        .info();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = resolve_remote_child_path(&request.parent_path, &request.name)?;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let mut file = sftp
                    .create(path.clone())
                    .await
                    .map_err(|error| format!("failed to create remote file '{path}': {error}"))?;
                file.shutdown()
                    .await
                    .map_err(|error| format!("failed to close remote file '{path}': {error}"))
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_read_file(
    state: tauri::State<'_, AppState>,
    request: SftpReadFileRequest,
) -> Result<String, String> {
    logging::event("terminal.sftp", "sftp.read_file.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &request.path)
        .debug();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = normalize_remote_path(&request.path);
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let metadata = sftp
                    .metadata(path.clone())
                    .await
                    .map_err(|error| format!("failed to stat remote file '{path}': {error}"))?;
                if metadata.file_type().is_dir() {
                    return Err(format!("remote path '{path}' is a directory"));
                }
                if metadata.len() > SFTP_EDIT_MAX_BYTES {
                    return Err(format!(
                        "remote file '{path}' is too large to edit in memory ({} bytes)",
                        metadata.len()
                    ));
                }

                let mut file = sftp
                    .open(path.clone())
                    .await
                    .map_err(|error| format!("failed to open remote file '{path}': {error}"))?;
                let mut bytes = Vec::with_capacity(metadata.len() as usize);
                file.read_to_end(&mut bytes)
                    .await
                    .map_err(|error| format!("failed to read remote file '{path}': {error}"))?;
                String::from_utf8(bytes)
                    .map_err(|error| format!("remote file '{path}' is not valid UTF-8: {error}"))
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_write_file(
    state: tauri::State<'_, AppState>,
    request: SftpWriteFileRequest,
) -> Result<SftpFileStatResult, String> {
    logging::event("terminal.sftp", "sftp.write_file.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &request.path)
        .field("bytes", request.content.len())
        .info();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = normalize_remote_path(&request.path);
    let content = request.content;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let mut file = sftp.create(path.clone()).await.map_err(|error| {
                    format!("failed to open remote file '{path}' for writing: {error}")
                })?;
                file.write_all(content.as_bytes())
                    .await
                    .map_err(|error| format!("failed to write remote file '{path}': {error}"))?;
                file.shutdown()
                    .await
                    .map_err(|error| format!("failed to close remote file '{path}': {error}"))?;
                let metadata = sftp.metadata(path.clone()).await.map_err(|error| {
                    format!("failed to stat remote file '{path}' after saving: {error}")
                })?;
                Ok(sftp_file_stat_result(path, &metadata))
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_stat_file(
    state: tauri::State<'_, AppState>,
    request: SftpStatFileRequest,
) -> Result<SftpFileStatResult, String> {
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let path = normalize_remote_path(&request.path);
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let metadata = sftp
                    .metadata(path.clone())
                    .await
                    .map_err(|error| format!("failed to stat remote file '{path}': {error}"))?;
                Ok(sftp_file_stat_result(path, &metadata))
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_rename(
    state: tauri::State<'_, AppState>,
    request: SftpRenameRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.rename")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("from_path", &request.from_path)
        .field("to_parent_path", &request.to_parent_path)
        .field("to_name", &request.to_name)
        .info();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let from_path = normalize_remote_path(&request.from_path);
    let to_path = resolve_remote_child_path(&request.to_parent_path, &request.to_name)?;
    let conflict_action = SftpNameConflictAction::parse(request.conflict_action.as_deref())?;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                rename_remote_path(sftp, &from_path, &to_path, conflict_action).await
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_choose_download_path(
    app: AppHandle,
    request: SftpChooseDownloadPathRequest,
) -> Result<Option<String>, String> {
    logging::event("terminal.sftp", "sftp.choose_download_path")
        .maybe_field("title", request.title.clone())
        .field("default_file_name", request.default_file_name.clone())
        .debug();
    choose_sftp_download_path(&app, &request).await
}

#[tauri::command]
pub(crate) async fn sftp_choose_upload_files(
    app: AppHandle,
    request: SftpChooseUploadFilesRequest,
) -> Result<Vec<String>, String> {
    logging::event("terminal.sftp", "sftp.choose_upload_files")
        .maybe_field("title", request.title.clone())
        .debug();
    choose_sftp_upload_files(&app, &request).await
}
