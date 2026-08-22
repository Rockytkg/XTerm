use tauri::AppHandle;

use crate::{
    logging,
    state::AppState,
    terminal::internal::{
        core::{
            SftpCloseSessionRequest, SftpTransferControlRequest, SftpTransferItem,
            SftpTransferRequest,
        },
        sftp::{
            cancel_sftp_transfer_task, list_sftp_transfer_tasks, pause_sftp_transfer,
            resume_sftp_transfer_task, start_sftp_transfer_task,
        },
    },
};

#[tauri::command]
pub(crate) async fn sftp_transfer(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SftpTransferRequest,
) -> Result<String, String> {
    logging::event("terminal.sftp", "sftp.transfer.start")
        .field("connection_id", &request.connection_id)
        .field("session_id", &request.session_id)
        .field("direction", &request.direction)
        .maybe_field("transfer_id", request.transfer_id.clone())
        .info();
    start_sftp_transfer_task(app, state.inner(), request).await
}

#[tauri::command]
pub(crate) async fn sftp_transfer_pause(request: SftpTransferControlRequest) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.transfer.pause")
        .field("transfer_id", &request.transfer_id)
        .info();
    pause_sftp_transfer(&request.transfer_id).await
}

#[tauri::command]
pub(crate) async fn sftp_transfer_resume(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SftpTransferControlRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.transfer.resume")
        .field("transfer_id", &request.transfer_id)
        .info();
    resume_sftp_transfer_task(app, state.inner(), &request.transfer_id).await
}

#[tauri::command]
pub(crate) async fn sftp_transfer_cancel(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SftpTransferControlRequest,
) -> Result<(), String> {
    logging::event("terminal.sftp", "sftp.transfer.cancel")
        .field("transfer_id", &request.transfer_id)
        .warn();
    cancel_sftp_transfer_task(app, state.inner(), &request.transfer_id).await
}

#[tauri::command]
pub(crate) async fn sftp_transfer_list(
    state: tauri::State<'_, AppState>,
    request: SftpCloseSessionRequest,
) -> Result<Vec<SftpTransferItem>, String> {
    list_sftp_transfer_tasks(state.inner(), &request.connection_id, &request.session_id).await
}
