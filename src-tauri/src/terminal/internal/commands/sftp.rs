use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use std::time::Duration;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

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
            delete_remote_path, ensure_remote_dir, join_remote_path, read_remote_file_bytes,
            remote_file_kind, remote_modified_timestamp, remote_parent_path, rename_remote_path,
            resolve_remote_child_path, resolve_remote_input_path, sftp_file_stat_result,
            sniff_remote_file_mime, sort_sftp_entries, SftpNameConflictAction, SFTP_EDIT_MAX_BYTES,
            SFTP_PREVIEW_MAX_BYTES,
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
    let result = sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let path = resolve_remote_input_path(sftp, &request.path).await?;
                let entries = sftp.read_dir(&path).await.map_err(|error| {
                    format!("failed to list remote directory '{path}': {error}")
                })?;
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
                Ok(SftpListResult {
                    parent: remote_parent_path(&path),
                    path,
                    entries,
                })
            })
        })
        .await?;
    logging::event("terminal.sftp", "sftp.list_remote.success")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &result.path)
        .field("entries", result.entries.len())
        .debug();
    Ok(result)
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
                    let resolved = resolve_remote_input_path(sftp, &path).await?;
                    delete_remote_path(sftp, &resolved).await?;
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
    let requested_path = request.path;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let (path, bytes) =
                    read_remote_file_bytes(sftp, &requested_path, SFTP_EDIT_MAX_BYTES, "edit")
                        .await?;
                String::from_utf8(bytes)
                    .map_err(|error| format!("remote file '{path}' is not valid UTF-8: {error}"))
            })
        })
        .await
}

#[tauri::command]
pub(crate) async fn sftp_read_file_base64(
    state: tauri::State<'_, AppState>,
    request: SftpReadFileRequest,
) -> Result<String, String> {
    logging::event("terminal.sftp", "sftp.read_file_base64.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("path", &request.path)
        .debug();
    let sftp_session =
        get_or_create_sftp_session(state.inner(), &request.connection_id, &request.session_id)
            .await?;
    let requested_path = request.path;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let (_, bytes) = read_remote_file_bytes(
                    sftp,
                    &requested_path,
                    SFTP_PREVIEW_MAX_BYTES,
                    "preview",
                )
                .await?;
                Ok(STANDARD_NO_PAD.encode(&bytes))
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
    let requested_path = request.path;
    let content = request.content;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let path = resolve_remote_input_path(sftp, &requested_path).await?;
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
                Ok(sftp_file_stat_result(path, &metadata, None))
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
    let requested_path = request.path;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let path = resolve_remote_input_path(sftp, &requested_path).await?;
                let metadata = sftp
                    .metadata(path.clone())
                    .await
                    .map_err(|error| format!("failed to stat remote file '{path}': {error}"))?;
                // 内容嗅探只为辅助预览分类，失败时静默降级为 None，不影响 stat 本身
                let mime = if metadata.file_type().is_file() {
                    match sniff_remote_file_mime(sftp, &path).await {
                        Ok(mime) => mime,
                        Err(error) => {
                            logging::event("terminal.sftp", "sftp.stat_file.sniff_failed")
                                .field("path", &path)
                                .field("error", &error)
                                .debug();
                            None
                        }
                    }
                } else {
                    None
                };
                Ok(sftp_file_stat_result(path, &metadata, mime))
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
    let from_path = request.from_path;
    let to_path = resolve_remote_child_path(&request.to_parent_path, &request.to_name)?;
    let conflict_action = SftpNameConflictAction::parse(request.conflict_action.as_deref())?;
    sftp_session
        .run_with_timeout(SFTP_COMMAND_TIMEOUT, move |sftp| {
            Box::pin(async move {
                let from_path = resolve_remote_input_path(sftp, &from_path).await?;
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
