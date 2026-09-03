use super::ssh_host_keys::*;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use parking_lot::Mutex;
use tauri::AppHandle;

use crate::{
    credentials::credential_secret_by_id,
    state::AppState,
    terminal::{
        domain::{ConnectionCapabilities, ProtocolKind},
        internal::{
            core::{
                ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
                TerminalSessionResources, TerminalSize, CONNECT_TIMEOUT_MS,
            },
            ssh_auth::{auth_from_secret, resolve_ssh_auth, SshAuth},
            ssh_client::{RusshClient, SharedSshSession, SshSessionTransport, SshShellTransport},
            telnet::normalize_terminal_type,
            terminal::{spawn_bound_session, BoundSessionOptions},
            util::{cancelable_open, ensure_open_current, ensure_open_not_cancelled, required},
        },
    },
    workspace::{workspace_connection_by_id, ConnectionProfile, JumpHostHop},
};

static PENDING_SSH_HOST_KEY_CONNECTIONS: OnceLock<Mutex<HashMap<String, SshHostKeyPending>>> =
    OnceLock::new();

struct ResolvedJumpHop {
    host: String,
    port: u16,
    username: String,
    auth: SshAuth,
}

struct SshHostKeyPending {
    session: russh::client::Handle<RusshClient>,
    chain_sessions: Vec<SharedSshSession>,
    host: String,
    port: u16,
    fingerprint: String,
}

struct SshConnected {
    session: russh::client::Handle<RusshClient>,
    channel: russh::Channel<russh::client::Msg>,
    chain_sessions: Vec<SharedSshSession>,
    host: String,
    port: u16,
    fingerprint: String,
}

enum SshConnectOutcome {
    Connected(SshConnected),
    HostKeyPrompt {
        pending: SshHostKeyPending,
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
}

enum SshOpenResolution {
    Connected(SshConnected),
    HostKeyPrompt(ConnectionOpenResult),
}

struct SshConnectParams {
    host: String,
    port: u16,
    username: String,
    auth: SshAuth,
    term: String,
    cols: u32,
    rows: u32,
    trusted_fingerprint: Option<String>,
    trust_host_key: bool,
    accept_host_key_once: bool,
}

fn pending_ssh_connections() -> &'static Mutex<HashMap<String, SshHostKeyPending>> {
    PENDING_SSH_HOST_KEY_CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 在 russh 默认算法之后追加 legacy 算法，兼容仅支持 ECDH、SHA-1 系列 KEX、
/// CBC/3DES、HMAC-SHA1 的旧交换机与服务器。列表顺序即协商优先级，
/// 现代服务器的协商结果与 russh 默认配置完全一致，legacy 算法仅在双方没有
/// 其他共同算法时兜底。
/// 注：未加入 ssh-dss 主机密钥——russh 的 `dsa` feature 与 ssh-key 0.7.0-rc.10
/// 组合编译失败（dsa 0.7.0 API 不兼容），且 1024 位 DSA 已被普遍淘汰。
fn ssh_client_config() -> russh::client::Config {
    let mut preferred = russh::Preferred::default();
    preferred.kex.to_mut().extend([
        russh::kex::ECDH_SHA2_NISTP256,
        russh::kex::ECDH_SHA2_NISTP384,
        russh::kex::ECDH_SHA2_NISTP521,
        russh::kex::DH_G14_SHA1,
        russh::kex::DH_G1_SHA1,
        russh::kex::DH_GEX_SHA1,
    ]);
    preferred.cipher.to_mut().extend([
        russh::cipher::AES_128_GCM,
        russh::cipher::AES_128_CBC,
        russh::cipher::AES_192_CBC,
        russh::cipher::AES_256_CBC,
        russh::cipher::TRIPLE_DES_CBC,
    ]);
    preferred
        .mac
        .to_mut()
        .extend([russh::mac::HMAC_SHA1_ETM, russh::mac::HMAC_SHA1]);
    russh::client::Config {
        preferred,
        // 旧设备 GEX 能提供的 DH 群上限常只有 2048 bit，下限取 russh 允许的最小值，
        // 避免对端没有符合要求的群可用（russh 默认 min 为 3072）。
        gex: russh::client::GexParams::new(2048, 8192, 8192).expect("valid gex params"),
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    }
}

fn normalize_jump_port(value: Option<&str>) -> u16 {
    value
        .and_then(|port| port.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(22)
}

fn filtered_jump_value(value: Option<&str>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_jump_hops(
    state: &AppState,
    request: &ResolvedConnection,
) -> Result<Vec<ResolvedJumpHop>, ConnectionError> {
    let mut stack = Vec::new();
    let mut hops = Vec::new();

    if let Some(items) = request.jump_hosts.as_ref() {
        for hop in items {
            hops.extend(resolve_jump_hop_chain(state, hop, &mut stack)?);
        }
    }

    Ok(hops)
}

fn resolve_jump_hop_chain(
    state: &AppState,
    hop: &JumpHostHop,
    stack: &mut Vec<String>,
) -> Result<Vec<ResolvedJumpHop>, ConnectionError> {
    if let Some(connection_id) = hop
        .connection_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return resolve_connection_jump_chain(state, connection_id, stack);
    }

    Ok(resolve_manual_jump_hop(state, hop)?.into_iter().collect())
}

fn resolve_connection_jump_chain(
    state: &AppState,
    connection_id: &str,
    stack: &mut Vec<String>,
) -> Result<Vec<ResolvedJumpHop>, ConnectionError> {
    if stack.iter().any(|id| id == connection_id) {
        let chain = stack.join(" -> ");
        return Err(ConnectionError::with_args(
            "jump_host_reference_loop",
            format!("jump host connection reference loop detected: {chain} -> {connection_id}"),
            serde_json::json!({ "chain": chain, "connectionId": connection_id }),
            false,
        ));
    }

    let profile = workspace_connection_by_id(state, connection_id)
        .map_err(|error| {
            ConnectionError::with_args(
                "jump_host_lookup_failed",
                error.clone(),
                serde_json::json!({ "detail": error }),
                false,
            )
        })?
        .ok_or_else(|| {
            ConnectionError::with_args(
                "jump_host_not_found",
                format!("jump host connection '{connection_id}' was not found"),
                serde_json::json!({ "connectionId": connection_id }),
                false,
            )
        })?;
    let name = profile.name.clone();
    let protocol = ProtocolKind::from_str(&profile.protocol).ok_or_else(|| {
        ConnectionError::with_args(
            "jump_host_protocol_invalid",
            format!("jump host connection '{name}' has an unsupported protocol"),
            serde_json::json!({ "name": name.clone() }),
            false,
        )
    })?;
    if protocol != ProtocolKind::Ssh {
        return Err(ConnectionError::with_args(
            "jump_host_not_ssh",
            format!("jump host connection '{name}' is not an SSH profile"),
            serde_json::json!({ "name": name }),
            false,
        ));
    }

    stack.push(connection_id.to_string());
    let result = resolve_profile_as_jump_chain(state, &profile, stack);
    stack.pop();
    result
}

fn resolve_profile_as_jump_chain(
    state: &AppState,
    profile: &ConnectionProfile,
    stack: &mut Vec<String>,
) -> Result<Vec<ResolvedJumpHop>, ConnectionError> {
    let mut hops = Vec::new();

    if let Some(items) = profile.jump_hosts() {
        for hop in items {
            hops.extend(resolve_jump_hop_chain(state, hop, stack)?);
        }
    }

    let name = profile.name.clone();
    let host = profile.host.trim().to_string();
    if host.is_empty() {
        return Err(ConnectionError::with_args(
            "jump_host_missing_host",
            format!("jump host connection '{name}' is missing a host"),
            serde_json::json!({ "name": name.clone() }),
            false,
        ));
    }
    let hop = JumpHostHop {
        connection_id: None,
        host,
        port: profile.port.clone(),
        user: Some(profile.user.clone()),
        auth_method: profile.auth_method().map(str::to_string),
        saved_credential_id: profile.saved_credential_id().map(str::to_string),
    };
    let resolved = resolve_manual_jump_hop(state, &hop)?.ok_or_else(|| {
        ConnectionError::with_args(
            "jump_host_incomplete",
            format!("jump host connection '{name}' is incomplete"),
            serde_json::json!({ "name": name }),
            false,
        )
    })?;
    hops.push(resolved);

    Ok(hops)
}

fn resolve_manual_jump_hop(
    state: &AppState,
    hop: &JumpHostHop,
) -> Result<Option<ResolvedJumpHop>, ConnectionError> {
    let host = match filtered_jump_value(Some(hop.host.as_str())) {
        Some(host) => host,
        None => return Ok(None),
    };
    let port = normalize_jump_port(hop.port.as_deref());
    let auth = resolve_jump_auth(state, hop, &host)?;
    let username = resolve_jump_username(hop, &host)?;
    Ok(Some(ResolvedJumpHop {
        host,
        port,
        username,
        auth,
    }))
}

fn resolve_jump_username(hop: &JumpHostHop, host: &str) -> Result<String, ConnectionError> {
    if let Some(user) = hop
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(user.to_string());
    }

    Err(ConnectionError::with_args(
        "jump_host_missing_username",
        format!("jump host '{host}' is missing a username"),
        serde_json::json!({ "host": host }),
        false,
    ))
}

fn resolve_jump_auth(
    state: &AppState,
    hop: &JumpHostHop,
    host: &str,
) -> Result<SshAuth, ConnectionError> {
    let credential_id = hop
        .saved_credential_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ConnectionError::with_args(
                "jump_host_missing_credential",
                format!("jump host '{host}' is missing its saved credential"),
                serde_json::json!({ "host": host }),
                false,
            )
        })?;
    let secret = credential_secret_by_id(state, credential_id)
        .map_err(|error| {
            ConnectionError::with_args(
                "jump_host_credential_lookup_failed",
                format!("jump host credential lookup failed: {error}"),
                serde_json::json!({ "detail": error.to_string() }),
                false,
            )
        })?
        .ok_or_else(|| {
            ConnectionError::with_args(
                "jump_host_credential_not_found",
                format!("jump host credential '{credential_id}' not found"),
                serde_json::json!({ "credentialId": credential_id }),
                false,
            )
        })?;
    auth_from_secret(secret).map_err(|error| {
        ConnectionError::with_args(
            "jump_host_auth_invalid",
            error.to_string(),
            serde_json::json!({ "detail": error.to_string() }),
            false,
        )
    })
}

fn store_pending_ssh_connection(connection_id: &str, pending: SshHostKeyPending) {
    let mut pending_connections = pending_ssh_connections().lock();
    if let Some(previous) = pending_connections.insert(connection_id.to_string(), pending) {
        disconnect_pending_ssh_connection(previous, "host key prompt superseded");
    }
}

fn take_pending_ssh_connection(connection_id: &str) -> Option<SshHostKeyPending> {
    pending_ssh_connections().lock().remove(connection_id)
}

fn disconnect_pending_ssh_connection(pending: SshHostKeyPending, reason: &'static str) {
    tauri::async_runtime::spawn(async move {
        let _ = pending
            .session
            .disconnect(russh::Disconnect::ByApplication, reason, "")
            .await;
        for session in pending.chain_sessions {
            let _ = session
                .lock()
                .await
                .disconnect(russh::Disconnect::ByApplication, reason, "")
                .await;
        }
    });
}

pub(crate) fn discard_pending_ssh_connection(connection_id: &str) {
    if let Some(pending) = pending_ssh_connections().lock().remove(connection_id) {
        disconnect_pending_ssh_connection(pending, "connection closed");
    }
}

async fn connect_ssh_async(params: SshConnectParams) -> ConnectionResult<SshConnectOutcome> {
    let SshConnectParams {
        host,
        port,
        username,
        auth,
        term,
        cols,
        rows,
        trusted_fingerprint,
        trust_host_key,
        accept_host_key_once,
    } = params;

    let host_key_state = Arc::new(Mutex::new(None));
    let config = Arc::new(ssh_client_config());
    let mut session = tokio::time::timeout(
        Duration::from_millis(CONNECT_TIMEOUT_MS),
        russh::client::connect(
            config,
            (host.as_str(), port),
            RusshClient::new(host_key_state.clone()),
        ),
    )
    .await
    .map_err(|_| {
        ConnectionError::with_args(
            "ssh_connect_timeout",
            format!("target={host}:{port}; connect timeout"),
            serde_json::json!({ "host": host, "port": port }),
            true,
        )
    })?
    .map_err(|error| {
        ConnectionError::with_args(
            "ssh_connect_failed",
            error.to_string(),
            serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
            false,
        )
    })?;

    let host_key = host_key_state.lock().clone().ok_or_else(|| {
        ConnectionError::new(
            "ssh_host_key_missing",
            "server did not provide a host key",
            false,
        )
    })?;
    log::info!(
        target: "ssh.connect",
        "ssh host key received for {host}:{port}: algorithm={}, fingerprint={}",
        host_key.algorithm,
        host_key.fingerprint
    );

    let key_trusted = trusted_fingerprint.as_deref() == Some(&host_key.fingerprint);
    if !key_trusted {
        log::warn!(target: "ssh.connect", "ssh host key is not trusted for {host}:{port}");
        if !trust_host_key && !accept_host_key_once {
            return Ok(SshConnectOutcome::HostKeyPrompt {
                pending: SshHostKeyPending {
                    session,
                    chain_sessions: Vec::new(),
                    host: host.clone(),
                    port,
                    fingerprint: host_key.fingerprint.clone(),
                },
                host,
                port,
                algorithm: host_key.algorithm,
                fingerprint: host_key.fingerprint,
            });
        }
        log::info!(target: "ssh.connect", "accepting ssh host key for {host}:{port}");
    }

    authenticate_russh_session(&mut session, &username, auth).await?;
    let channel = open_authenticated_shell_channel(&mut session, &term, cols, rows).await?;

    Ok(SshConnectOutcome::Connected(SshConnected {
        session,
        channel,
        chain_sessions: Vec::new(),
        host,
        port,
        fingerprint: host_key.fingerprint,
    }))
}

async fn connect_first_jump_hop(hop: ResolvedJumpHop) -> ConnectionResult<SharedSshSession> {
    let config = Arc::new(ssh_client_config());
    let host_key_state = Arc::new(Mutex::new(None));
    let host = hop.host.clone();
    let port = hop.port;
    let mut session = russh::client::connect(
        config,
        (hop.host.as_str(), hop.port),
        RusshClient::new(host_key_state),
    )
    .await
    .map_err(|error| {
        ConnectionError::with_args(
            "ssh_jump_host_connect_failed",
            format!("jump_host={host}:{port}; {error}"),
            serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
            false,
        )
    })?;
    authenticate_russh_session(&mut session, &hop.username, hop.auth).await?;
    Ok(Arc::new(tokio::sync::Mutex::new(session)))
}

async fn connect_next_jump_hop(
    upstream: SharedSshSession,
    hop: ResolvedJumpHop,
) -> ConnectionResult<SharedSshSession> {
    let host = hop.host.clone();
    let port = hop.port;
    let stream = {
        let upstream = upstream.lock().await;
        let channel = upstream
            .channel_open_direct_tcpip(hop.host.as_str(), hop.port as u32, "127.0.0.1", 0u32)
            .await
            .map_err(|error| ConnectionError::with_args(
                "ssh_jump_tunnel_failed",
                format!("target={host}:{port}; {error}"),
                serde_json::json!({ "host": host.clone(), "port": port, "detail": error.to_string() }),
                false,
            ))?;
        channel.into_stream()
    };
    let config = Arc::new(ssh_client_config());
    let host_key_state = Arc::new(Mutex::new(None));
    let mut session =
        russh::client::connect_stream(config, stream, RusshClient::new(host_key_state))
            .await
            .map_err(|error| {
                ConnectionError::with_args(
                    "ssh_jump_host_connect_failed",
                    format!("jump_host={host}:{port}; via_chain=true; {error}"),
                    serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
                    false,
                )
            })?;
    authenticate_russh_session(&mut session, &hop.username, hop.auth).await?;
    Ok(Arc::new(tokio::sync::Mutex::new(session)))
}

async fn connect_jump_chain(
    hops: Vec<ResolvedJumpHop>,
) -> ConnectionResult<(SharedSshSession, Vec<SharedSshSession>)> {
    let mut iter = hops.into_iter();
    let first = iter.next().ok_or_else(|| {
        ConnectionError::new(
            "jump_host_required",
            "at least one jump host is required",
            false,
        )
    })?;
    let mut chain_sessions = Vec::new();
    let mut current = connect_first_jump_hop(first).await?;
    chain_sessions.push(current.clone());
    for hop in iter {
        current = connect_next_jump_hop(current, hop).await?;
        chain_sessions.push(current.clone());
    }
    Ok((current, chain_sessions))
}

async fn connect_via_jump_chain(
    jump_hops: Vec<ResolvedJumpHop>,
    params: SshConnectParams,
) -> ConnectionResult<SshConnectOutcome> {
    let SshConnectParams {
        host,
        port,
        username,
        auth,
        term,
        cols,
        rows,
        trusted_fingerprint,
        trust_host_key,
        accept_host_key_once,
    } = params;

    let chain_label = jump_hops
        .iter()
        .map(|hop| format!("{}:{}", hop.host, hop.port))
        .collect::<Vec<_>>()
        .join(" -> ");
    let (session, chain_sessions) = connect_jump_chain(jump_hops).await?;

    let host_key_state = Arc::new(Mutex::new(None));
    let config = Arc::new(ssh_client_config());
    let tunnel_channel =
        {
            let jump_session = session.lock().await;
            let channel = jump_session
            .channel_open_direct_tcpip(host.as_str(), port as u32, "127.0.0.1", 0u32)
            .await
            .map_err(|error| ConnectionError::with_args(
                "ssh_jump_tunnel_failed",
                format!("target={host}:{port}; {error}"),
                serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
                false,
            ))?;
            channel.into_stream()
        };

    let mut session = russh::client::connect_stream(
        config,
        tunnel_channel,
        RusshClient::new(host_key_state.clone()),
    )
    .await
    .map_err(|error| ConnectionError::with_args(
        "ssh_connect_failed",
        format!("target={host}:{port}; jump_chain={chain_label}; {error}"),
        serde_json::json!({ "host": host, "port": port, "chainLabel": chain_label, "detail": error.to_string() }),
        false,
    ))?;

    let host_key = host_key_state.lock().clone().ok_or_else(|| {
        ConnectionError::new(
            "ssh_host_key_missing",
            "target server did not provide a host key",
            false,
        )
    })?;

    let key_trusted = trusted_fingerprint.as_deref() == Some(&host_key.fingerprint);
    if !key_trusted && !trust_host_key && !accept_host_key_once {
        return Ok(SshConnectOutcome::HostKeyPrompt {
            pending: SshHostKeyPending {
                session,
                chain_sessions,
                host: host.clone(),
                port,
                fingerprint: host_key.fingerprint.clone(),
            },
            host,
            port,
            algorithm: host_key.algorithm,
            fingerprint: host_key.fingerprint,
        });
    }

    authenticate_russh_session(&mut session, &username, auth).await?;
    let channel = open_authenticated_shell_channel(&mut session, &term, cols, rows).await?;

    Ok(SshConnectOutcome::Connected(SshConnected {
        session,
        channel,
        chain_sessions,
        host,
        port,
        fingerprint: host_key.fingerprint,
    }))
}

async fn open_authenticated_shell_channel(
    session: &mut russh::client::Handle<RusshClient>,
    term: &str,
    cols: u32,
    rows: u32,
) -> ConnectionResult<russh::Channel<russh::client::Msg>> {
    let channel = session.channel_open_session().await.map_err(|error| {
        ConnectionError::with_args(
            "ssh_channel_open_failed",
            error.to_string(),
            serde_json::json!({ "detail": error.to_string() }),
            false,
        )
    })?;
    channel
        .request_pty(true, term, cols, rows, 0, 0, &[])
        .await
        .map_err(|error| {
            ConnectionError::with_args(
                "ssh_pty_request_failed",
                error.to_string(),
                serde_json::json!({ "detail": error.to_string() }),
                false,
            )
        })?;
    channel.request_shell(true).await.map_err(|error| {
        ConnectionError::with_args(
            "ssh_shell_request_failed",
            error.to_string(),
            serde_json::json!({ "detail": error.to_string() }),
            false,
        )
    })?;
    Ok(channel)
}

async fn connect_pending_ssh_async(
    mut pending: SshHostKeyPending,
    username: &str,
    auth: SshAuth,
    term: &str,
    cols: u32,
    rows: u32,
) -> ConnectionResult<SshConnected> {
    authenticate_russh_session(&mut pending.session, username, auth).await?;
    let channel = open_authenticated_shell_channel(&mut pending.session, term, cols, rows).await?;
    Ok(SshConnected {
        session: pending.session,
        channel,
        chain_sessions: pending.chain_sessions,
        host: pending.host,
        port: pending.port,
        fingerprint: pending.fingerprint,
    })
}

async fn connect_or_jump_chain(
    jump_hops: Vec<ResolvedJumpHop>,
    params: SshConnectParams,
) -> ConnectionResult<SshConnectOutcome> {
    if jump_hops.is_empty() {
        connect_ssh_async(params).await
    } else {
        connect_via_jump_chain(jump_hops, params).await
    }
}

async fn connect_new_ssh_session(
    state: &AppState,
    request: &ResolvedConnection,
    params: SshConnectParams,
) -> ConnectionResult<SshOpenResolution> {
    let outcome = connect_or_jump_chain(resolve_jump_hops(state, request)?, params).await?;
    Ok(match outcome {
        SshConnectOutcome::Connected(connected) => SshOpenResolution::Connected(connected),
        SshConnectOutcome::HostKeyPrompt {
            pending,
            host,
            port,
            algorithm,
            fingerprint,
        } => {
            ensure_open_not_cancelled(request)?;
            store_pending_ssh_connection(&request.id, pending);
            SshOpenResolution::HostKeyPrompt(ConnectionOpenResult::HostKeyPrompt {
                host,
                port,
                algorithm,
                fingerprint,
            })
        }
    })
}

pub(crate) async fn authenticate_russh_session(
    session: &mut russh::client::Handle<RusshClient>,
    username: &str,
    auth: SshAuth,
) -> ConnectionResult<()> {
    let result = match auth {
        SshAuth::Password(password) => session
            .authenticate_password(username, password)
            .await
            .map_err(|error| {
                ConnectionError::with_args(
                    "ssh_password_auth_failed",
                    error.to_string(),
                    serde_json::json!({ "detail": error.to_string() }),
                    false,
                )
            })?,
        SshAuth::Key {
            private_key,
            passphrase,
        } => {
            let key = russh::keys::decode_secret_key(&private_key, passphrase.as_deref()).map_err(
                |error| {
                    ConnectionError::with_args(
                        "ssh_key_decode_failed",
                        error.to_string(),
                        serde_json::json!({ "detail": error.to_string() }),
                        false,
                    )
                },
            )?;
            let hash_alg = session.best_supported_rsa_hash().await.map_err(|error| {
                ConnectionError::with_args(
                    "ssh_key_algorithm_failed",
                    error.to_string(),
                    serde_json::json!({ "detail": error.to_string() }),
                    false,
                )
            })?;
            session
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg.flatten()),
                )
                .await
                .map_err(|error| {
                    ConnectionError::with_args(
                        "ssh_key_auth_failed",
                        error.to_string(),
                        serde_json::json!({ "detail": error.to_string() }),
                        false,
                    )
                })?
        }
    };

    if result.success() {
        return Ok(());
    }

    Err(ConnectionError::new(
        "ssh_auth_rejected",
        "server rejected credential",
        false,
    ))
}

pub(crate) struct SshConnectionFactory;

impl SshConnectionFactory {
    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ResolvedConnection,
    ) -> ConnectionResult<ConnectionOpenResult> {
        let host = required(request.host.as_deref(), "host")
            .map_err(|error| {
                ConnectionError::with_args(
                    "ssh_host_required",
                    error.clone(),
                    serde_json::json!({ "detail": error }),
                    false,
                )
            })?
            .to_string();
        let port = request.port.unwrap_or(22);
        log::info!(target: "ssh.connect", "opening async ssh connection to {host}:{port}");

        let username = request
            .user
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                ConnectionError::new("ssh_user_required", "SSH user is required", false)
            })?
            .to_string();
        let has_inline_credential = request
            .inline_password
            .as_deref()
            .is_some_and(|v| !v.is_empty())
            || request
                .inline_private_key
                .as_deref()
                .is_some_and(|v| !v.is_empty());
        let has_saved_credential = request
            .saved_credential_id
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty());
        if !has_inline_credential && !has_saved_credential {
            return Err(ConnectionError::new(
            "ssh_credential_required",
            "SSH connection requires a credential — provide a password in the URI, or select a saved credential", false));
        }
        let auth = resolve_ssh_auth(state, &request)?;
        let trusted_fingerprint = state.store().host_key(&request.id).ok().flatten();
        let accept_once_trusted =
            state.has_host_key_trusted_once_for_connection(&request.id, &host, port);
        let trusted_fp_for_task = if accept_once_trusted {
            state
                .trusted_once_fingerprint_for_connection(&request.id, &host, port)
                .or(trusted_fingerprint)
        } else {
            trusted_fingerprint
        };

        let trust_host_key = request.trust_host_key.unwrap_or(false);
        let accept_host_key_once = request.accept_host_key_once.unwrap_or(false);
        let term = normalize_terminal_type(request.terminal_type.as_deref());
        let cols = request.cols.unwrap_or(80).clamp(1, 1_000);
        let rows = request.rows.unwrap_or(24).clamp(1, 1_000);

        let pending = if trust_host_key || accept_host_key_once {
            take_pending_ssh_connection(&request.id).and_then(|pending| {
                if pending.host == host && pending.port == port {
                    Some(pending)
                } else {
                    disconnect_pending_ssh_connection(pending, "host key decision did not match");
                    None
                }
            })
        } else {
            if let Some(pending) = take_pending_ssh_connection(&request.id) {
                disconnect_pending_ssh_connection(pending, "host key prompt restarted");
            }
            None
        };

        let connected = if let Some(pending) = pending {
            cancelable_open(
                &request,
                connect_pending_ssh_async(pending, &username, auth, &term, cols, rows),
            )
            .await?
        } else {
            match cancelable_open(
                &request,
                connect_new_ssh_session(
                    state,
                    &request,
                    SshConnectParams {
                        host,
                        port,
                        username,
                        auth,
                        term,
                        cols,
                        rows,
                        trusted_fingerprint: trusted_fp_for_task,
                        trust_host_key,
                        accept_host_key_once,
                    },
                ),
            )
            .await?
            {
                SshOpenResolution::Connected(connected) => connected,
                SshOpenResolution::HostKeyPrompt(prompt) => return Ok(prompt),
            }
        };

        if trust_host_key {
            if let Err(error) = save_host_key(state, &request.id, &connected.fingerprint) {
                log::error!(
                    target: "ssh.connect",
                    "failed to persist trusted SSH host key for connection {}: {error}",
                    request.id
                );
            }
        } else if accept_host_key_once {
            state.trust_host_key_once_for_connection(
                &request.id,
                &connected.host,
                connected.port,
                &connected.fingerprint,
            );
        }

        let open_context = request.session_open_context(state);
        ensure_open_current(state, &request)?;
        // 运行时指标默认开启；会话选项可关闭——仅支持单通道 shell 的设备
        //（部分交换机）会在 exec 探测时被远端断开整个会话。
        let mut capabilities = ConnectionCapabilities::ssh();
        capabilities.metrics = request.runtime_metrics.unwrap_or(true);
        let shared_session = Arc::new(tokio::sync::Mutex::new(connected.session));
        let session_id = spawn_bound_session(
            app.clone(),
            state,
            BoundSessionOptions {
                session_prefix: "ssh",
                connection_id: open_context.connection_id,
                transport: Box::new(SshSessionTransport {
                    transport: SshShellTransport {
                        session: shared_session.clone(),
                        channel: connected.channel,
                    },
                    initial_size: TerminalSize { cols, rows },
                }),
                capabilities,
                codec: open_context.codec,
                initial_data: None,
                startup_auth: None,
                resources: TerminalSessionResources::ssh(shared_session, connected.chain_sessions),
                replay_line_limit: open_context.replay_line_limit,
            },
        );
        log::info!(
            target: "ssh.connect",
            "validated async SSH connection '{}' to {}:{}",
            session_id,
            connected.host,
            connected.port
        );
        Ok(ConnectionOpenResult::connected_shell(
            session_id,
            ProtocolKind::Ssh,
            open_context.encoding_label,
        ))
    }
}
