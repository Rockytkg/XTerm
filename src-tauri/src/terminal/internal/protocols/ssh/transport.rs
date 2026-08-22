use bytes::Bytes;

use super::{
    core::{SessionWorkerEvent, TransportCommand, TransportCommandOutcome, WRITE_STALL_TIMEOUT},
    ssh_client::{SharedSshSession, SshShellTransport},
    transport_events::{
        resolve_unsupported_transport_capability, send_transport_closed, send_transport_data,
        send_transport_failed, send_transport_ready, write_all_and_flush_transport,
    },
};

pub(super) fn spawn_ssh_transport_actor(
    session_id: String,
    transport: SshShellTransport,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    tauri::async_runtime::spawn(async move {
        if !send_transport_ready(&event_tx) {
            return;
        }
        log::debug!(target: "ssh.transport", "backend SSH actor for '{session_id}' started");
        let (mut read_half, write_half) = transport.channel.split();
        let mut writer = write_half.make_writer();

        loop {
            tokio::select! {
                command = rx.recv() => {
                    match command {
                        Some(command) => {
                            match handle_ssh_transport_command(command, &mut writer, &write_half).await {
                                Ok(TransportCommandOutcome::Continue) => {}
                                Ok(TransportCommandOutcome::Close) => {
                                    let _ = disconnect_ssh_session(&transport.session).await;
                                    send_transport_closed(&event_tx, None);
                                    return;
                                }
                                Err(error) => {
                                    let _ = disconnect_ssh_session(&transport.session).await;
                                    send_transport_failed(&event_tx, error);
                                    return;
                                }
                            }
                        }
                        None => {
                            let _ = write_half.close().await;
                            let _ = disconnect_ssh_session(&transport.session).await;
                            return;
                        }
                    }
                }
                message = read_half.wait() => {
                    match message {
                        Some(russh::ChannelMsg::Data { data }) | Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                            if !send_transport_data(
                                &event_tx,
                                Bytes::copy_from_slice(&data),
                                None,
                            ) {
                                let _ = write_half.close().await;
                                let _ = disconnect_ssh_session(&transport.session).await;
                                return;
                            }
                        }
                        Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                            send_transport_closed(
                                &event_tx,
                                Some("SSH channel closed by remote host".to_string()),
                            );
                            return;
                        }
                        Some(russh::ChannelMsg::ExitStatus { exit_status }) => {
                            if exit_status == 0 {
                                send_transport_closed(&event_tx, Some(format!(
                                    "SSH shell exited with status {exit_status}"
                                )));
                            } else {
                                send_transport_failed(&event_tx, format!(
                                    "SSH shell exited with status {exit_status}"
                                ));
                            };
                            return;
                        }
                        Some(russh::ChannelMsg::ExitSignal { signal_name, error_message, .. }) => {
                            let reason = if error_message.is_empty() {
                                format!("SSH shell exited by signal {signal_name:?}")
                            } else {
                                format!("SSH shell exited by signal {signal_name:?}: {error_message}")
                            };
                            send_transport_failed(&event_tx, reason);
                            return;
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    });
}

async fn disconnect_ssh_session(session: &SharedSshSession) -> Result<(), russh::Error> {
    session
        .lock()
        .await
        .disconnect(russh::Disconnect::ByApplication, "", "")
        .await
}

async fn handle_ssh_transport_command(
    command: TransportCommand,
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    write_half: &russh::ChannelWriteHalf<russh::client::Msg>,
) -> Result<TransportCommandOutcome, String> {
    match command {
        TransportCommand::Write(bytes) => {
            // A stalled write means the channel is wedged; fail the transport
            // so the session is torn down instead of blocking the actor.
            tokio::time::timeout(
                WRITE_STALL_TIMEOUT,
                write_all_and_flush_transport(writer, &bytes, "SSH channel"),
            )
            .await
            .map_err(|_| "SSH channel write timed out".to_string())??;
            Ok(TransportCommandOutcome::Continue)
        }
        TransportCommand::InvokeCapability(command) => {
            resolve_unsupported_transport_capability(command)
        }
        TransportCommand::Resize(resize) => {
            if let Err(error) = write_half
                .window_change(
                    resize.cols,
                    resize.rows,
                    resize.width_px.unwrap_or(0),
                    resize.height_px.unwrap_or(0),
                )
                .await
            {
                log::debug!(target: "ssh.transport", "failed to resize SSH PTY: {error}");
            }
            Ok(TransportCommandOutcome::Continue)
        }
        TransportCommand::Close => {
            let _ = write_half.close().await;
            Ok(TransportCommandOutcome::Close)
        }
    }
}
