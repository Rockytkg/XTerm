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
    port: tokio_serial::SerialStream,
    port_name: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    close_ack: tokio::sync::oneshot::Sender<()>,
) {
    tauri::async_runtime::spawn(async move {
        let mut port = port;
        run_serial_transport_actor(session_id, &mut port, &port_name, &mut rx, &event_tx).await;
        // POSIX 下串口以 TIOCEXCL + 独占 flock 打开:必须先 drop 释放 fd,
        // 再确认关闭,否则紧接着重开同一端口会得到 EBUSY(端口被占用)。
        drop(port);
        let _ = close_ack.send(());
    });
}

async fn run_serial_transport_actor(
    session_id: String,
    port: &mut tokio_serial::SerialStream,
    port_name: &str,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    if !send_transport_ready(event_tx) {
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
                match handle_serial_transport_command(port_name, port, command, event_tx).await {
                    Ok(TransportCommandOutcome::Continue) => {}
                    Ok(TransportCommandOutcome::Close) => {
                        send_transport_closed(event_tx, None);
                        return;
                    }
                    Err(error) => {
                        send_transport_failed(event_tx, error);
                        return;
                    }
                }
            }
            result = read_serial_transport(port, &mut buffer, &mut aborted_read_recovery_available) => {
                match result {
                    Ok(size) if size > 0 => {
                        // 一次成功读取说明重叠 IO 健康,重新武装 aborted 恢复名额。
                        aborted_read_recovery_available = true;
                        log_serial_read_chunk(port_name, &buffer[..size]);
                        let bytes = buffer.split_to(size).freeze();
                        buffer.resize(SERIAL_READ_BATCH_MAX_BYTES, 0);
                        if !send_transport_data(event_tx, bytes, None) {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        send_transport_failed(event_tx, error);
                        return;
                    }
                }
            }
        };
    }
}

/// Read one batch of serial input: wait for the first byte(s), then drain
/// everything already queued so one wakeup delivers a whole chunk.
///
/// Waiting for the first byte is platform-specific:
/// - Unix goes through `AsyncRead`, which drives the fd via `AsyncFd::try_io`
///   and therefore clears/waits epoll readiness correctly. tokio-serial's own
///   `readable()` drops the readiness guard without clearing it, so once the
///   port becomes readable (or the open-time probe leaves stale readiness
///   behind) it resolves immediately forever and the actor spins instead of
///   sleeping until the next byte.
/// - Windows keeps `readable()` + `try_read`: the handle is driven by
///   overlapped IO, and starting (then cancelling) an `AsyncRead` future can
///   leave a stale ERROR_OPERATION_ABORTED completion queued.
async fn read_serial_transport(
    port: &mut tokio_serial::SerialStream,
    buffer: &mut [u8],
    aborted_read_recovery_available: &mut bool,
) -> Result<usize, String> {
    let arrived = wait_serial_input(port, buffer).await?;
    drain_ready_serial_input(port, buffer, arrived, aborted_read_recovery_available)
}

#[cfg(unix)]
async fn wait_serial_input(
    port: &mut tokio_serial::SerialStream,
    buffer: &mut [u8],
) -> Result<usize, String> {
    use tokio::io::AsyncReadExt;

    port.read(buffer)
        .await
        .map_err(|error| format!("failed to read serial data: {error}"))
}

#[cfg(windows)]
async fn wait_serial_input(
    port: &mut tokio_serial::SerialStream,
    _buffer: &mut [u8],
) -> Result<usize, String> {
    port.readable()
        .await
        .map_err(|error| format!("failed to wait for serial data: {error}"))?;
    Ok(0)
}

/// Non-blocking drain of bytes that are already queued. Returns the total
/// batch size (`arrived` plus what the drain picked up).
fn drain_ready_serial_input(
    port: &mut tokio_serial::SerialStream,
    buffer: &mut [u8],
    arrived: usize,
    aborted_read_recovery_available: &mut bool,
) -> Result<usize, String> {
    let mut total = arrived;
    while total < buffer.len() {
        match port.try_read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(size) => total += size,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
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
            // 已读到的数据先交付;错误留给下一轮等待时上报。
            Err(error) if total > 0 => {
                log::debug!(target: "serial.transport", "serial read stopped after {total} byte(s): {error}");
                break;
            }
            Err(error) => return Err(format!("failed to read serial data: {error}")),
        }
    }
    Ok(total)
}

/// ERROR_OPERATION_ABORTED (995) only exists on Windows.
fn is_windows_operation_aborted(error: &std::io::Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(995)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_windows_operation_aborted_without_hiding_other_errors() {
        let aborted = std::io::Error::from_raw_os_error(995);
        let denied = std::io::Error::from_raw_os_error(5);

        assert_eq!(is_windows_operation_aborted(&aborted), cfg!(windows));
        assert!(!is_windows_operation_aborted(&denied));
    }

    #[test]
    fn hex_preview_marks_truncation() {
        assert_eq!(serial_hex_preview(&[0x01, 0xAB], 8), "01 AB");
        assert_eq!(serial_hex_preview(&[0x01, 0xAB], 1), "01 ...");
        assert_eq!(serial_hex_preview(&[], 8), "");
    }
}
