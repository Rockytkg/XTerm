use std::{fmt::Write as _, time::Duration};

use bytes::{Bytes, BytesMut};
use tokio::io::AsyncWriteExt;

use super::{
    core::{
        SessionWorkerEvent, TransportCapabilityCommand, TransportCommand, TransportCommandOutcome,
        SERIAL_READ_BATCH_MAX_BYTES, SERIAL_WRITE_STALL_TIMEOUT_MS,
    },
    serial::redetect_serial_baud_on_open_port,
    transport_events::{
        send_transport_closed, send_transport_data, send_transport_failed, send_transport_ready,
    },
};

const SERIAL_READ_LOG_PREVIEW_BYTES: usize = 64;

pub(super) fn spawn_serial_transport_actor(
    session_id: String,
    mut port: tokio_serial::SerialStream,
    port_name: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    tauri::async_runtime::spawn(async move {
        if !send_transport_ready(&event_tx) {
            return;
        }
        let mut buffer = BytesMut::zeroed(SERIAL_READ_BATCH_MAX_BYTES);
        let mut aborted_read_recovery_available = true;
        log::debug!(target: "serial.transport", "backend serial actor for '{session_id}' started on {port_name}");
        loop {
            tokio::select! {
                biased;
                command = rx.recv() => {
                    let Some(command) = command else {
                        return;
                    };
                    match handle_serial_transport_command(&port_name, &mut port, command, &event_tx).await {
                        Ok(TransportCommandOutcome::Continue) => {}
                        Ok(TransportCommandOutcome::Close) => {
                            send_transport_closed(&event_tx, None);
                            return;
                        }
                        Err(error) => {
                            send_transport_failed(&event_tx, error);
                            return;
                        }
                    }
                }
                readiness = port.readable() => {
                    let result = match readiness {
                        Ok(()) => read_ready_serial_transport(
                            &mut port,
                            &mut buffer,
                            &mut aborted_read_recovery_available,
                        ).await,
                        Err(error) => handle_serial_readiness_error(&error).await,
                    };
                    match result {
                        Ok(size) if size > 0 => {
                            aborted_read_recovery_available = true;
                            log_serial_read_chunk(&port_name, &buffer[..size]);
                            let bytes = buffer.split_to(size).freeze();
                            buffer.resize(SERIAL_READ_BATCH_MAX_BYTES, 0);
                            if !send_transport_data(&event_tx, bytes, None) {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            send_transport_failed(&event_tx, error);
                            return;
                        }
                    }
                }
            };
        }
    });
}

async fn handle_serial_transport_command(
    port_name: &str,
    port: &mut tokio_serial::SerialStream,
    command: TransportCommand,
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) -> Result<TransportCommandOutcome, String> {
    match command {
        TransportCommand::Write(bytes) => {
            write_serial_transport(port, &bytes).await?;
            Ok(TransportCommandOutcome::Continue)
        }
        TransportCommand::InvokeCapability(command) => {
            handle_serial_transport_capability(port_name, port, command, event_tx).await
        }
        TransportCommand::Resize(_) => Ok(TransportCommandOutcome::Continue),
        TransportCommand::Close => Ok(TransportCommandOutcome::Close),
    }
}

async fn handle_serial_transport_capability(
    port_name: &str,
    port: &mut tokio_serial::SerialStream,
    command: TransportCapabilityCommand,
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) -> Result<TransportCommandOutcome, String> {
    match command {
        TransportCapabilityCommand::RedetectSerialBaud { encoding, reply } => {
            let mut result =
                redetect_serial_baud_on_open_port(port_name, port, encoding.as_deref()).await;
            if let Ok(result) = result.as_mut() {
                let sample = std::mem::take(&mut result.initial_sample);
                if !sample.is_empty() && !send_transport_data(event_tx, Bytes::from(sample), None) {
                    return Ok(TransportCommandOutcome::Close);
                }
            }
            let _ = reply.send(result.map_err(|error| error.detail));
            Ok(TransportCommandOutcome::Continue)
        }
    }
}

async fn write_serial_transport(
    port: &mut tokio_serial::SerialStream,
    bytes: &[u8],
) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_millis(SERIAL_WRITE_STALL_TIMEOUT_MS),
        port.write_all(bytes),
    )
    .await
    .map_err(|_| "serial write timed out".to_string())?
    .map_err(|error| format!("failed to write serial data: {error}"))
}

async fn read_ready_serial_transport(
    port: &mut tokio_serial::SerialStream,
    buffer: &mut [u8],
    aborted_read_recovery_available: &mut bool,
) -> Result<usize, String> {
    let mut total = 0;
    let limit = buffer.len().min(SERIAL_READ_BATCH_MAX_BYTES);

    loop {
        if total >= limit {
            return Ok(total);
        }

        match port.try_read(&mut buffer[total..limit]) {
            Ok(0) => return Ok(total),
            Ok(size) => {
                total += size;
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Ok(total);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error)
                if *aborted_read_recovery_available && is_windows_operation_aborted(&error) =>
            {
                // PurgeComm or a cancelled readiness operation can leave one
                // ERROR_OPERATION_ABORTED completion queued on Windows. Consume
                // that stale completion once; a repeated abort remains fatal.
                *aborted_read_recovery_available = false;
                log::debug!(target: "serial.transport", "recovered one aborted serial read completion: {error}");
                continue;
            }
            Err(error) if total > 0 => {
                log::debug!(target: "serial.transport", "serial read stopped after {total} byte(s): {error}");
                return Ok(total);
            }
            Err(error) => return Err(format!("failed to read serial data: {error}")),
        }
    }
}

fn is_windows_operation_aborted(error: &std::io::Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(995)
}

fn log_serial_read_chunk(port_name: &str, bytes: &[u8]) {
    if !log::log_enabled!(target: "serial.transport", log::Level::Trace) {
        return;
    }

    let preview = serial_hex_preview(bytes, SERIAL_READ_LOG_PREVIEW_BYTES);
    log::trace!(
        target: "serial.transport",
        "serial read port='{port_name}' bytes={} preview_hex={preview}",
        bytes.len()
    );
}

fn serial_hex_preview(bytes: &[u8], limit: usize) -> String {
    let shown = bytes.len().min(limit);
    let mut output = String::with_capacity(shown.saturating_mul(3) + 16);

    for (index, byte) in bytes.iter().take(shown).enumerate() {
        if index > 0 {
            output.push(' ');
        }
        let _ = write!(&mut output, "{byte:02X}");
    }

    if bytes.len() > shown {
        output.push_str(" ...");
    }

    output
}

async fn handle_serial_readiness_error(error: &std::io::Error) -> Result<usize, String> {
    Err(format!("failed to wait for serial data: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn active_serial_abort_is_reported_as_a_transport_failure() {
        let error = std::io::Error::from_raw_os_error(995);

        let result = handle_serial_readiness_error(&error).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .starts_with("failed to wait for serial data:"));
    }

    #[test]
    fn identifies_windows_operation_aborted_without_hiding_other_errors() {
        let aborted = std::io::Error::from_raw_os_error(995);
        let denied = std::io::Error::from_raw_os_error(5);

        assert_eq!(is_windows_operation_aborted(&aborted), cfg!(windows));
        assert!(!is_windows_operation_aborted(&denied));
    }
}
