//! VNC connection factory: TCP connect, RFB handshake, loopback bridge setup.
//!
//! The VNC session itself carries no terminal data — the frontend's RFB
//! client renders the remote desktop directly from the WebSocket bridge.
//! The backend session exists to own the connection lifecycle (state machine
//! events, close propagation) and the bridge.

use std::time::Duration;

use tauri::AppHandle;

use crate::{
    credentials::credential_secret_by_id,
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            core::{
                ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
                TerminalSessionResources, VncBridgeInfo, CONNECT_TIMEOUT_MS,
            },
            loopback_bridge::bind_loopback_bridge,
            terminal::{spawn_bound_session, BoundSessionOptions},
            util::{cancelable_open, ensure_open_current, ensure_open_not_cancelled, required},
        },
    },
};

mod bridge;
mod handshake;

use bridge::VncSessionTransport;
use handshake::{probe_server_handshake, ServerHandshake};

pub(crate) struct VncConnectionFactory;

impl VncConnectionFactory {
    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ResolvedConnection,
    ) -> ConnectionResult<ConnectionOpenResult> {
        let host = required(request.host.as_deref(), "host")
            .map_err(|error| {
                ConnectionError::with_args(
                    "vnc_host_required",
                    error.clone(),
                    serde_json::json!({ "detail": error }),
                    false,
                )
            })?
            .to_string();
        let port = request.port.unwrap_or(5900);
        let target = format!("{host}:{port}");
        crate::logging::event("terminal.vnc", "vnc.open.start")
            .field("target", &target)
            .info();

        let mut stream = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                tokio::net::TcpStream::connect((host.as_str(), port)),
            )
            .await
            .map_err(|_| {
                ConnectionError::with_args(
                    "vnc_connect_timeout",
                    format!("target={target}; connect timeout"),
                    serde_json::json!({ "host": host, "port": port }),
                    true,
                )
            })?
            .map_err(|error| connect_failed_error(&target, &host, port, &error))
        })
        .await?;
        stream
            .set_nodelay(true)
            .map_err(|error| connect_failed_error(&target, &host, port, &error))?;

        let password = resolve_vnc_password(state, &request)?;
        let shared = request.vnc_shared.unwrap_or(true);
        let handshake = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                probe_server_handshake(&mut stream, password.as_deref(), shared, &target),
            )
            .await
            .map_err(|_| {
                ConnectionError::with_args(
                    "vnc_handshake_failed",
                    format!("target={target}; handshake timeout"),
                    serde_json::json!({ "host": host, "port": port }),
                    true,
                )
            })?
        })
        .await?;

        let bridge = bind_loopback_bridge("vnc")
            .await
            .map_err(|error| bridge_failed_error(&error))?;

        let bridge_info = match &handshake {
            ServerHandshake::Terminated(terminated) => VncBridgeInfo {
                url: bridge.url.clone(),
                desktop_name: Some(terminated.desktop_name.clone()),
                width: u32::from(terminated.width),
                height: u32::from(terminated.height),
                auth_passthrough: false,
            },
            ServerHandshake::Passthrough(_) => VncBridgeInfo {
                url: bridge.url.clone(),
                desktop_name: None,
                width: 0,
                height: 0,
                auth_passthrough: true,
            },
        };

        ensure_open_not_cancelled(&request)?;
        ensure_open_current(state, &request)?;
        let open_context = request.session_open_context(state);
        let session_id = spawn_bound_session(
            app,
            state,
            BoundSessionOptions {
                session_prefix: "vnc",
                connection_id: open_context.connection_id,
                transport: Box::new(VncSessionTransport {
                    listener: bridge.listener,
                    token: bridge.token,
                    server: stream,
                    handshake,
                }),
                capabilities: ConnectionCapabilities::vnc(),
                codec: open_context.codec,
                initial_data: None,
                startup_auth: None,
                resources: TerminalSessionResources::default(),
                replay_line_limit: open_context.replay_line_limit,
            },
        );
        crate::logging::event("terminal.vnc", "vnc.open.success")
            .field("target", &target)
            .field("session_id", &session_id)
            .field("auth_passthrough", bridge_info.auth_passthrough)
            .info();
        Ok(ConnectionOpenResult::connected_vnc(session_id, bridge_info))
    }
}

/// Resolves the VNC password: an inline password (deep-link or auth retry)
/// wins over the saved credential. Returns `None` when no password is
/// available; servers offering security=None still connect in that case.
fn resolve_vnc_password(
    state: &AppState,
    request: &ResolvedConnection,
) -> ConnectionResult<Option<String>> {
    if let Some(password) = request
        .inline_password
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(password.to_string()));
    }
    let Some(credential_id) = request
        .saved_credential_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let secret = credential_secret_by_id(state, credential_id)
        .map_err(|error| {
            ConnectionError::with_args(
                "vnc_credential_failed",
                error.clone(),
                serde_json::json!({ "detail": error }),
                false,
            )
        })?
        .ok_or_else(|| {
            ConnectionError::with_args(
                "vnc_credential_failed",
                format!("selected credential '{credential_id}' does not exist"),
                serde_json::json!({ "credentialId": credential_id }),
                false,
            )
        })?;
    if secret.cred_type() != "password" {
        return Err(ConnectionError::with_args(
            "vnc_credential_failed",
            format!("saved credential '{credential_id}' cannot be used for VNC authentication"),
            serde_json::json!({ "credentialId": credential_id }),
            false,
        ));
    }
    let password = secret
        .password()
        .ok_or_else(|| {
            ConnectionError::with_args(
                "vnc_credential_failed",
                "saved password credential is missing its password",
                serde_json::json!({ "credentialId": credential_id }),
                false,
            )
        })?
        .to_string();
    if password.is_empty() {
        return Ok(None);
    }
    Ok(Some(password))
}

fn connect_failed_error(
    target: &str,
    host: &str,
    port: u16,
    error: &std::io::Error,
) -> ConnectionError {
    ConnectionError::with_args(
        "vnc_connect_failed",
        format!("target={target}; {error}"),
        serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
        true,
    )
}

fn bridge_failed_error(error: &std::io::Error) -> ConnectionError {
    ConnectionError::with_args(
        "vnc_bridge_failed",
        format!("failed to start VNC bridge: {error}"),
        serde_json::json!({ "detail": error.to_string() }),
        true,
    )
}
