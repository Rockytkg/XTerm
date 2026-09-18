//! Loopback WebSocket ↔ TCP bridge for VNC sessions.
//!
//! The frontend RFB client (noVNC) cannot open raw TCP sockets, so each VNC
//! session owns a WebSocket listener on 127.0.0.1 that splices the frontend
//! onto the VNC server's TCP stream. Access is gated by a per-session random
//! token in the URL query so no other local process can hijack the bridge.
//!
//! In "terminated" mode the backend already completed the RFB handshake with
//! the server; the bridge replays a synthetic security=None handshake to the
//! frontend and then splices bytes. In "passthrough" mode (server offers
//! only security types the backend cannot answer) the buffered server
//! handshake is replayed and the stream is forwarded untouched.

use bytes::{Buf, Bytes};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::HeaderValue,
        Message,
    },
    WebSocketStream,
};

use crate::terminal::internal::core::{
    SessionTransportRuntime, SessionWorkerEvent, TerminalSize, TransportCommand,
};

use super::handshake::ServerHandshake;

/// Bytes the frontend sends before the streams are spliced and that must be
/// swallowed because the backend already sent them to the server:
/// terminated mode = client version (12) + security selection (1) +
/// ClientInit (1); passthrough mode = client version (12).
const TERMINATED_CLIENT_PREAMBLE_LEN: usize = 14;
const PASSTHROUGH_CLIENT_PREAMBLE_LEN: usize = 12;

pub(super) struct VncSessionTransport {
    pub listener: TcpListener,
    pub token: String,
    pub server: TcpStream,
    pub handshake: ServerHandshake,
}

impl SessionTransportRuntime for VncSessionTransport {
    fn initial_size(&self) -> Option<TerminalSize> {
        None
    }

    fn spawn(
        self: Box<Self>,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
        event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    ) {
        tokio::spawn(async move {
            log::debug!(target: "terminal.vnc", "vnc bridge actor started for session {session_id}");
            run_bridge_actor(*self, rx, event_tx).await;
        });
    }
}

struct BridgeClient {
    ws: WebSocketStream<TcpStream>,
    swallow: usize,
}

async fn run_bridge_actor(
    transport: VncSessionTransport,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    let VncSessionTransport {
        listener,
        token,
        mut server,
        handshake,
    } = transport;
    let passthrough = matches!(handshake, ServerHandshake::Passthrough(_));
    let _ = event_tx.send(SessionWorkerEvent::Ready);

    let mut client: Option<BridgeClient> = None;
    // Passthrough mode can only replay the server handshake once; a second
    // client cannot be re-negotiated with the real server.
    let mut passthrough_handshake_replayed = false;
    // Server bytes read while no frontend client is attached; delivered to the
    // next client after its (replayed) handshake.
    let mut pending_server_bytes: Vec<u8> = Vec::new();
    let mut read_buffer = vec![0_u8; 64 * 1024];

    loop {
        tokio::select! {
            command = rx.recv() => {
                match command {
                    // Terminal writes/resizes do not apply to the video path;
                    // the RFB client talks to the server through this bridge.
                    Some(TransportCommand::Close) | None => break,
                    Some(_) => {}
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => match accept_bridge_client(stream, &token).await {
                        Ok(ws) => {
                            match prepare_client(
                                ws,
                                &handshake,
                                &mut pending_server_bytes,
                                passthrough && passthrough_handshake_replayed,
                            )
                            .await
                            {
                                Ok(next) => {
                                    passthrough_handshake_replayed = true;
                                    client = Some(next);
                                }
                                Err(error) => log::warn!(target: "terminal.vnc", "vnc bridge client setup failed: {error}"),
                            }
                        }
                        Err(error) => log::warn!(target: "terminal.vnc", "vnc bridge websocket accept failed: {error}"),
                    },
                    Err(error) => {
                        let _ = event_tx.send(SessionWorkerEvent::Failed(format!(
                            "VNC bridge listener failed: {error}"
                        )));
                        break;
                    }
                }
            }
            read = server.read(&mut read_buffer) => {
                match read {
                    Ok(0) => {
                        let _ = event_tx.send(SessionWorkerEvent::Closed(Some(
                            "VNC server closed the connection".to_string(),
                        )));
                        break;
                    }
                    Ok(size) => {
                        if let Some(active) = client.as_mut() {
                            let payload = Bytes::copy_from_slice(&read_buffer[..size]);
                            if active.ws.send(Message::Binary(payload)).await.is_err() {
                                client = None;
                                if passthrough {
                                    let _ = event_tx.send(SessionWorkerEvent::Closed(Some(
                                        "VNC client disconnected".to_string(),
                                    )));
                                    break;
                                }
                            }
                        } else {
                            pending_server_bytes.extend_from_slice(&read_buffer[..size]);
                        }
                    }
                    Err(error) => {
                        let _ = event_tx.send(SessionWorkerEvent::Failed(format!(
                            "VNC server connection failed: {error}"
                        )));
                        break;
                    }
                }
            }
            message = async { client.as_mut().expect("guarded by precondition").ws.next().await }, if client.is_some() => {
                match message {
                    Some(Ok(Message::Binary(data))) => {
                        let Some(active) = client.as_mut() else { continue };
                        let mut data = data;
                        if active.swallow > 0 {
                            let skip = active.swallow.min(data.len());
                            active.swallow -= skip;
                            data.advance(skip);
                        }
                        if !data.is_empty() && server.write_all(&data).await.is_err() {
                            let _ = event_tx.send(SessionWorkerEvent::Failed(
                                "VNC server connection write failed".to_string(),
                            ));
                            break;
                        }
                    }
                    // The frontend closed (or errored). Terminated sessions
                    // stay alive for a re-attach; a passthrough session cannot
                    // redo the server-side handshake and ends here.
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        client = None;
                        if passthrough {
                            let _ = event_tx.send(SessionWorkerEvent::Closed(Some(
                                "VNC client disconnected".to_string(),
                            )));
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    let _ = server.shutdown().await;
}

// ErrorResponse is tungstenite's callback signature; boxing it is not an option.
#[allow(clippy::result_large_err)]
async fn accept_bridge_client(
    stream: TcpStream,
    token: &str,
) -> Result<WebSocketStream<TcpStream>, String> {
    let _ = stream.set_nodelay(true);
    let expected = format!("token={token}");
    let callback = |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
        let query = request.uri().query().unwrap_or("");
        if !query.split('&').any(|part| part == expected) {
            return Err(http_forbidden());
        }
        let offers_binary = request
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(',').any(|entry| entry.trim() == "binary"))
            .unwrap_or(false);
        if offers_binary {
            response
                .headers_mut()
                .append("Sec-WebSocket-Protocol", HeaderValue::from_static("binary"));
        }
        Ok(response)
    };
    tokio_tungstenite::accept_hdr_async(stream, callback)
        .await
        .map_err(|error| format!("websocket handshake failed: {error}"))
}

fn http_forbidden() -> ErrorResponse {
    tokio_tungstenite::tungstenite::http::Response::builder()
        .status(403)
        .body(Some("forbidden".to_string()))
        .expect("static 403 response is valid")
}

/// Sends the replayed handshake to a freshly connected frontend client and
/// returns the client state with the number of client preamble bytes to
/// swallow before splicing the streams.
async fn prepare_client(
    mut ws: WebSocketStream<TcpStream>,
    handshake: &ServerHandshake,
    pending_server_bytes: &mut Vec<u8>,
    passthrough_reattach: bool,
) -> Result<BridgeClient, String> {
    let swallow = match handshake {
        ServerHandshake::Terminated(terminated) => {
            // Emulate an RFB 3.8 server that requires no authentication; the
            // real handshake (including any password auth) already happened
            // between the backend and the server.
            send_binary(&mut ws, b"RFB 003.008\n").await?;
            send_binary(&mut ws, &[1, 1]).await?; // one security type: None
            send_binary(&mut ws, &[0, 0, 0, 0]).await?; // SecurityResult: OK
            send_binary(&mut ws, &terminated.server_init).await?;
            TERMINATED_CLIENT_PREAMBLE_LEN
        }
        ServerHandshake::Passthrough(buffered) => {
            if passthrough_reattach {
                return Err("passthrough VNC bridge cannot re-run the server handshake".to_string());
            }
            send_binary(&mut ws, buffered).await?;
            PASSTHROUGH_CLIENT_PREAMBLE_LEN
        }
    };
    if !pending_server_bytes.is_empty() {
        send_binary(&mut ws, &std::mem::take(pending_server_bytes)).await?;
    }
    Ok(BridgeClient { ws, swallow })
}

async fn send_binary(ws: &mut WebSocketStream<TcpStream>, bytes: &[u8]) -> Result<(), String> {
    ws.send(Message::Binary(Bytes::copy_from_slice(bytes)))
        .await
        .map_err(|error| format!("failed to send handshake replay: {error}"))
}
