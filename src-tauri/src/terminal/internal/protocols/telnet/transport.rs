use bytes::Bytes;

use super::{
    core::{
        SessionWorkerEvent, TransportCommand, TransportCommandOutcome, SESSION_BUFFER_SIZE,
        WRITE_STALL_TIMEOUT,
    },
    telnet::TelnetRuntime,
    transport_events::{
        resolve_unsupported_transport_capability, send_transport_closed, send_transport_data,
        send_transport_failed, send_transport_ready,
    },
};

pub(super) fn spawn_telnet_transport_actor(
    session_id: String,
    mut runtime: Box<TelnetRuntime>,
    rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    tauri::async_runtime::spawn(async move {
        let mut rx = rx;
        let mut buffer = vec![0_u8; SESSION_BUFFER_SIZE];
        let mut transport_ready = false;
        log::debug!(target: "telnet.transport", "backend Telnet actor for '{session_id}' started");
        loop {
            tokio::select! {
                biased;
                command = rx.recv() => {
                    let Some(command) = command else {
                        let _ = runtime.close().await;
                        return;
                    };
                    match handle_telnet_transport_command(&mut runtime, command).await {
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
                result = runtime.read_into_with_negotiation(&mut buffer) => {
                    match result {
                        Ok((size, became_ready)) => {
                            if became_ready && !transport_ready && !send_transport_ready(&event_tx) {
                                let _ = runtime.close().await;
                                return;
                            }
                            transport_ready |= became_ready;
                            if size == 0 && !became_ready {
                                continue;
                            }
                            if send_transport_data(
                                &event_tx,
                                Bytes::copy_from_slice(&buffer[..size]),
                                None,
                            ) {
                                continue;
                            }
                            let _ = runtime.close().await;
                            return;
                        }
                        Err(error) => {
                            send_telnet_transport_failure(&event_tx, &error, transport_ready);
                            return;
                        }
                    }
                }
            }
        }
    });
}

fn send_telnet_transport_failure(
    event_tx: &tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    error: &str,
    transport_ready: bool,
) {
    if error == "Telnet connection closed by remote host" && transport_ready {
        send_transport_closed(event_tx, Some(error.to_string()));
    } else if error == "Telnet connection closed by remote host" {
        send_transport_failed(
            event_tx,
            "Telnet negotiation failed: remote host closed the connection before the session became ready",
        );
    } else {
        send_transport_failed(event_tx, error);
    }
}

async fn handle_telnet_transport_command(
    runtime: &mut TelnetRuntime,
    command: TransportCommand,
) -> Result<TransportCommandOutcome, String> {
    match command {
        TransportCommand::Write(bytes) => {
            // Align with the serial transport: a stalled write fails the
            // transport instead of blocking the actor forever.
            tokio::time::timeout(WRITE_STALL_TIMEOUT, runtime.write(&bytes))
                .await
                .map_err(|_| "telnet write timed out".to_string())??;
            Ok(TransportCommandOutcome::Continue)
        }
        TransportCommand::InvokeCapability(command) => {
            resolve_unsupported_transport_capability(command)
        }
        TransportCommand::Resize(resize) => {
            runtime.resize(resize.cols, resize.rows).await?;
            Ok(TransportCommandOutcome::Continue)
        }
        TransportCommand::Close => {
            runtime.close().await?;
            Ok(TransportCommandOutcome::Close)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_eof_before_negotiation_is_failed() {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        send_telnet_transport_failure(&event_tx, "Telnet connection closed by remote host", false);

        match event_rx.try_recv().unwrap() {
            SessionWorkerEvent::Failed(detail) => {
                assert!(detail.starts_with("Telnet negotiation failed:"));
            }
            _ => panic!("expected failed event"),
        }
    }

    #[test]
    fn remote_eof_after_negotiation_is_closed() {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        send_telnet_transport_failure(&event_tx, "Telnet connection closed by remote host", true);

        match event_rx.try_recv().unwrap() {
            SessionWorkerEvent::Closed(Some(detail)) => {
                assert_eq!(detail, "Telnet connection closed by remote host");
            }
            _ => panic!("expected closed event"),
        }
    }

    #[test]
    fn readiness_is_a_distinct_lifecycle_event() {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        assert!(send_transport_ready(&event_tx));

        assert!(matches!(event_rx.try_recv(), Ok(SessionWorkerEvent::Ready)));
    }
}
