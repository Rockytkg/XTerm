use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::core::{SessionWorkerEvent, TransportCapabilityCommand, TransportCommandOutcome};

pub(super) fn send_transport_ready(
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) -> bool {
    event_tx.send(SessionWorkerEvent::Ready).is_ok()
}

pub(super) fn send_transport_data(
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    bytes: impl Into<Bytes>,
    negotiated_encoding: Option<String>,
) -> bool {
    event_tx
        .send(SessionWorkerEvent::Data {
            bytes: bytes.into(),
            negotiated_encoding,
        })
        .is_ok()
}

pub(super) fn send_transport_closed(
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    reason: Option<String>,
) {
    let _ = event_tx.send(SessionWorkerEvent::Closed(reason));
}

pub(super) fn send_transport_failed(
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    error: impl Into<String>,
) {
    let _ = event_tx.send(SessionWorkerEvent::Failed(error.into()));
}

pub(super) fn resolve_unsupported_transport_capability(
    command: TransportCapabilityCommand,
) -> Result<TransportCommandOutcome, String> {
    match command {
        TransportCapabilityCommand::RedetectSerialBaud { reply, .. } => {
            let _ = reply.send(Err(
                "serial baud redetect is only supported for serial sessions".to_string(),
            ));
            Ok(TransportCommandOutcome::Continue)
        }
    }
}

pub(super) async fn write_all_transport(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    context: &str,
) -> Result<(), String> {
    writer
        .write_all(bytes)
        .await
        .map_err(|error| format!("failed to write {context} data: {error}"))
}

pub(super) async fn write_all_and_flush_transport(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    context: &str,
) -> Result<(), String> {
    write_all_transport(writer, bytes, context).await?;
    writer
        .flush()
        .await
        .map_err(|error| format!("failed to flush {context} data: {error}"))
}
