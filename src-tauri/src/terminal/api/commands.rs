use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use tauri::{ipc::Channel, AppHandle};

use crate::{
    logging,
    state::AppState,
    terminal::{
        api::dto::*,
        app::SessionResizeRequest,
        connection_service,
        domain::TerminalApiError,
        internal::{ConnectionOpenRequest, SshRuntimeMetricsRequest},
        protocol::protocol_registry,
        session_service,
    },
};

#[tauri::command]
pub(crate) async fn terminal_connection_open(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: ConnectionOpenCommand,
) -> Result<ConnectionOpenResponse, String> {
    connection_service()
        .open(
            app,
            state.inner(),
            ConnectionOpenRequest {
                connection_id: request.connection_id,
                open_request_id: request.open_request_id,
                trust_host_key: request.trust_host_key,
                accept_host_key_once: request.accept_host_key_once,
                terminal_scrollback: request.terminal_scrollback,
                terminal_type: request.terminal_type,
                encoding: request.encoding,
                realtime_encoding_detection: request.realtime_encoding_detection,
                cols: request.cols,
                rows: request.rows,
                ssh_credential: request.ssh_credential,
            },
        )
        .await
        .map_err(api_error)
}

#[tauri::command]
pub(crate) async fn terminal_connection_authenticate(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: ConnectionAuthenticateCommand,
) -> Result<ConnectionOpenResponse, String> {
    connection_service()
        .authenticate(app, state.inner(), request)
        .await
        .map_err(api_error)
}

#[tauri::command]
pub(crate) fn terminal_connection_close(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: ConnectionCloseCommand,
) -> Result<(), String> {
    connection_service()
        .close(app, state.inner(), &request.connection_id)
        .map_err(api_error)
}

#[tauri::command]
pub(crate) fn terminal_connection_open_cancel(
    state: tauri::State<'_, AppState>,
    request: ConnectionOpenCancelCommand,
) -> Result<(), String> {
    state.inner().cancel_connection_open(
        request
            .open_request_id
            .as_deref()
            .unwrap_or(&request.connection_id),
    );
    protocol_registry().discard_pending_connections(&request.connection_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn terminal_session_close(
    _app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SessionCloseCommand,
) -> Result<(), String> {
    session_service()
        .close(state.inner(), &request.session_id)
        .map_err(api_error)?;
    state.remove_terminal_output_channels(&request.session_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn terminal_session_set_encoding_detection(
    state: tauri::State<'_, AppState>,
    request: SessionEncodingDetectionCommand,
) -> Result<(), String> {
    session_service()
        .set_encoding_detection(
            state.inner(),
            &request.session_id,
            request.channel_id,
            request.enabled,
            request.encoding,
        )
        .map_err(api_error)
}

#[tauri::command]
pub(crate) async fn terminal_serial_redetect_baud(
    state: tauri::State<'_, AppState>,
    request: SessionSerialRedetectCommand,
) -> Result<crate::terminal::internal::SerialRedetectResult, String> {
    session_service()
        .redetect_serial_baud(state.inner(), &request.session_id)
        .await
        .map_err(api_error)
}

#[tauri::command]
pub(crate) async fn terminal_metrics_start(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SshRuntimeMetricsRequest,
) -> Result<(), String> {
    connection_service()
        .start_metrics(app, state.inner(), request)
        .await
        .map_err(api_error)
}

#[tauri::command]
pub(crate) fn terminal_metrics_stop(
    state: tauri::State<'_, AppState>,
    request: SshRuntimeMetricsRequest,
) -> Result<(), String> {
    connection_service()
        .stop_metrics(state.inner(), request)
        .map_err(api_error)
}

#[tauri::command]
pub(crate) async fn terminal_attach(
    _app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: String,
    channel: Channel<TerminalSessionChannelPayload>,
) -> Result<SessionAttachResult, String> {
    if session_id.trim().is_empty() {
        return Err("session id is required".to_string());
    }
    let subscription_id = state.reserve_terminal_output_subscription(&session_id, channel);
    let lease = session_service()
        .activate_with_reserved_subscription(state.inner(), &session_id, Some(subscription_id))
        .await
        .map_err(|error| {
            state.release_terminal_output_subscription(&session_id, subscription_id);
            api_error(error)
        })?;
    // Startup replay can be emitted immediately after the worker accepts Activate.
    // Bind again here for already-active sessions; new channels were bound before activation.
    if !state.bind_terminal_output_lease(&session_id, subscription_id, lease.channel_id) {
        cleanup_failed_attach(
            state.inner(),
            &session_id,
            subscription_id,
            Some(lease.channel_id),
        )
        .await;
        return Err(
            "terminal output subscription was removed before activation completed".to_string(),
        );
    }
    if let Err(error) = session_service().flush_output(state.inner(), &session_id) {
        cleanup_failed_attach(
            state.inner(),
            &session_id,
            subscription_id,
            Some(lease.channel_id),
        )
        .await;
        return Err(api_error(error));
    }
    Ok(SessionAttachResult {
        accepted: true,
        session_id,
        connection_id: lease.connection_id,
        channel_id: lease.channel_id,
        already_active: lease.already_active,
        subscription_id,
    })
}

#[tauri::command]
pub(crate) async fn terminal_detach(
    _app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: SessionDetachCommand,
) -> Result<(), String> {
    session_service()
        .deactivate(state.inner(), &request.session_id, request.channel_id)
        .await
        .map_err(api_error)?;
    if let Some(subscription_id) = request.subscription_id {
        state.release_terminal_output_subscription(&request.session_id, subscription_id);
    }
    Ok(())
}

async fn cleanup_failed_attach(
    state: &AppState,
    session_id: &str,
    subscription_id: u64,
    channel_id: Option<u64>,
) {
    state.release_terminal_output_subscription(session_id, subscription_id);
    if let Some(channel_id) = channel_id {
        let _ = session_service()
            .deactivate(state, session_id, Some(channel_id))
            .await;
    }
}

#[tauri::command]
pub(crate) fn terminal_send_batch(
    state: tauri::State<'_, AppState>,
    frames: Vec<TerminalClientFrame>,
) -> Result<(), String> {
    for frame in frames {
        handle_terminal_client_frame(state.inner(), frame).map_err(api_error)?;
    }
    Ok(())
}

fn handle_terminal_client_frame(
    state: &AppState,
    frame: TerminalClientFrame,
) -> Result<(), TerminalApiError> {
    match frame {
        TerminalClientFrame::InputText {
            session_id,
            channel_id,
            input_sequence,
            data,
        } => session_service().write(state, &session_id, channel_id, input_sequence, data),
        TerminalClientFrame::InputBytes {
            session_id,
            channel_id,
            input_sequence,
            data_base64,
        } => {
            let data = STANDARD_NO_PAD
                .decode(data_base64.as_bytes())
                .map_err(|e| TerminalApiError::invalid(format!("invalid base64 data: {e}")))?;
            session_service().write_bytes(state, &session_id, channel_id, input_sequence, data)
        }
        TerminalClientFrame::Resize {
            session_id,
            channel_id,
            cols,
            rows,
            width_px,
            height_px,
        } => session_service().resize(
            state,
            &session_id,
            SessionResizeRequest {
                channel_id,
                cols,
                rows,
                width_px,
                height_px,
            },
        ),
        TerminalClientFrame::RawOutput {
            session_id,
            channel_id,
            enabled,
        } => session_service().set_raw_output(state, &session_id, channel_id, enabled),
        TerminalClientFrame::RenderedOffset {
            session_id,
            channel_id,
            offset,
        } => session_service().rendered_offset(state, &session_id, Some(channel_id), offset),
    }
}

fn api_error(error: TerminalApiError) -> String {
    logging::event("terminal.api", "terminal.request.failed")
        .field("error_code", error.error_code.clone())
        .field("recoverable", error.recoverable)
        .field("message", error.message.clone())
        .maybe_field("detail", error.detail.clone())
        .warn();
    serde_json::to_string(&error).unwrap_or_else(|serialize_error| {
        serde_json::json!({
            "errorCode": "INTERNAL_ERROR",
            "recoverable": true,
            "detail": format!("failed to serialize terminal API error: {serialize_error}"),
        })
        .to_string()
    })
}
