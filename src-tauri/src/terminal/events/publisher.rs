use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{
    state::AppState,
    terminal::{
        api::dto::{
            ConnectionHostKeyChallengePayload, SessionStatusChangedPayload, TerminalEventEnvelope,
        },
        internal::core::{SftpTransferProgressEvent, SshRuntimeMetrics},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionLifecycle {
    Ready,
    Failed,
    Closed,
}

impl SessionLifecycle {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

pub(crate) fn emit_terminal_event<T: Serialize + Clone>(
    app: &AppHandle,
    name: &str,
    payload: T,
) -> Result<(), String> {
    app.emit("terminal-event", TerminalEventEnvelope::new(name, payload))
        .map_err(|error| error.to_string())
}

pub(crate) fn emit_session_lifecycle(
    app: &AppHandle,
    state: &AppState,
    connection_id: &str,
    session_id: &str,
    lifecycle: SessionLifecycle,
    detail: Option<String>,
) -> Result<(), String> {
    let next_state = lifecycle.as_str();
    let previous_state = state
        .session_runtime(session_id)
        .map(|value| value.lifecycle_state)
        .unwrap_or_else(|| "pending".to_string());
    state.set_session_lifecycle(session_id, next_state.to_string());
    let payload = SessionStatusChangedPayload {
        connection_id: connection_id.to_string(),
        session_id: session_id.to_string(),
        state: next_state.to_string(),
        detail,
    };
    if previous_state != payload.state {
        emit_terminal_event(app, "session.status_changed", payload)?;
    }
    Ok(())
}

pub(crate) fn emit_connection_host_key_challenge(
    app: &AppHandle,
    payload: ConnectionHostKeyChallengePayload,
) -> Result<(), String> {
    emit_terminal_event(app, "connection.host_key_challenge", payload)
}

pub(crate) fn emit_sftp_transfer_progress(
    app: &AppHandle,
    transfer_id: &str,
    transferred: u64,
    total: u64,
    done: bool,
    error: Option<String>,
) {
    let payload = SftpTransferProgressEvent {
        transfer_id: transfer_id.to_string(),
        transferred,
        total,
        done,
        status: error
            .as_ref()
            .map(|_| "failed".to_string())
            .or_else(|| done.then(|| "done".to_string())),
        error,
    };
    let _ = app.emit(
        "terminal-event",
        TerminalEventEnvelope::new("sftp.transfer_progress", payload),
    );
}

pub(crate) fn emit_sftp_transfer_status(
    app: &AppHandle,
    transfer_id: &str,
    transferred: u64,
    total: u64,
    done: bool,
    status: &str,
    error: Option<String>,
) {
    let payload = SftpTransferProgressEvent {
        transfer_id: transfer_id.to_string(),
        transferred,
        total,
        done,
        status: Some(status.to_string()),
        error,
    };
    let _ = app.emit(
        "terminal-event",
        TerminalEventEnvelope::new("sftp.transfer_progress", payload),
    );
}

pub(crate) fn emit_ssh_runtime_metrics(app: &AppHandle, metrics: SshRuntimeMetrics) {
    let _ = app.emit(
        "terminal-event",
        TerminalEventEnvelope::new("metrics.report", metrics),
    );
}
