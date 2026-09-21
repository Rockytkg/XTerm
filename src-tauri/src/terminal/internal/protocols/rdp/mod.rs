//! RDP connection factory: TCP connect, TLS/CredSSP（NLA）协商, 会话建立与回环桥启动。
//!
//! 与 VNC 相同，RDP 会话本身不走终端数据通道——后端用 IronRDP 完成整个协议状态机，
//! 前端经回环 WebSocket 桥收发帧/输入（见 `codec.rs` 的帧协议）。后端会话负责
//! 生命周期（状态机事件、关闭传播）与桥的所有权。
//!
//! TLS 证书策略 v1 为接受任意证书（IronRDP 内置 accept-all），与 mstsc 首次连接的
//! "信任并继续"语义一致；不做指纹确认。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::AppHandle;

use ironrdp::connector::{
    ClientConnector, Config, ConnectorError, ConnectorErrorKind, Credentials, DesktopSize,
    ServerName,
};
use ironrdp::displaycontrol::client::DisplayControlClient;
use ironrdp::dvc::DrdynvcClient;
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::rdp::client_info::{CompressionType, PerformanceFlags, TimezoneInfo};

use crate::{
    credentials::credential_secret_by_id,
    state::AppState,
    terminal::{
        domain::ConnectionCapabilities,
        internal::{
            core::{
                ConnectionError, ConnectionOpenResult, ConnectionResult, RdpBridgeInfo,
                ResolvedConnection, TerminalSessionResources, CONNECT_TIMEOUT_MS,
            },
            loopback_bridge::bind_loopback_bridge,
            terminal::{spawn_bound_session, BoundSessionOptions},
            util::{cancelable_open, ensure_open_current, ensure_open_not_cancelled, required},
        },
    },
};

mod codec;
mod session;

use session::{BridgeClipboardBackend, ClipboardEvent, RdpSessionTransport};

/// 初始桌面尺寸；开启动态分辨率（resize_session）后由前端视口尺寸驱动调整。
const INITIAL_DESKTOP_WIDTH: u16 = 1920;
const INITIAL_DESKTOP_HEIGHT: u16 = 1080;

pub(crate) struct RdpConnectionFactory;

impl RdpConnectionFactory {
    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ResolvedConnection,
    ) -> ConnectionResult<ConnectionOpenResult> {
        let host = required(request.host.as_deref(), "host")
            .map_err(|error| {
                ConnectionError::with_args(
                    "rdp_host_required",
                    error.clone(),
                    serde_json::json!({ "detail": error }),
                    false,
                )
            })?
            .to_string();
        let port = request.port.unwrap_or(3389);
        // RDP 没有"先连上再问用户名"的图形登录等价物放在后端：NLA 在会话建立前
        // 就需要完整凭据，用户名缺失时无法继续。
        let username = required(request.user.as_deref(), "username")
            .map_err(|error| {
                ConnectionError::with_args(
                    "rdp_username_required",
                    error.clone(),
                    serde_json::json!({ "detail": error }),
                    false,
                )
            })?
            .to_string();
        let target = format!("{host}:{port}");
        crate::logging::event("terminal.rdp", "rdp.open.start")
            .field("target", &target)
            .field("user", &username)
            // scale_mode 由前端渲染层执行，后端只透传记录，便于排查会话配置。
            .maybe_field("scale_mode", request.rdp_scale_mode.clone())
            .info();

        let password = resolve_rdp_password(state, &request)?;
        let has_password = password.is_some();
        let clipboard_sync = request.rdp_clipboard_sync.unwrap_or(true);
        // 与前端默认值对齐：动态分辨率默认关、剪贴板同步默认开。
        let resize_session = request.rdp_resize_session.unwrap_or(false);

        let stream = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                tokio::net::TcpStream::connect((host.as_str(), port)),
            )
            .await
            .map_err(|_| {
                ConnectionError::with_args(
                    "rdp_connect_timeout",
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
        let client_addr = stream
            .local_addr()
            .map_err(|error| connect_failed_error(&target, &host, port, &error))?;

        let config = build_connector_config(&request, username, password.unwrap_or_default());
        let mut connector = ClientConnector::new(config, client_addr);
        // 剪贴板 SVC 与动态分辨率 DVC 只能在连接序列里注册、会话开始后无法追加，
        // 因此一律注册（mstsc 亦然）；clipboard_sync / resize_session 仅作为前端
        // 运行时门控，侧边栏开关随改随生效。服务器不接受对应通道时相关路径自然静默。
        let (clipboard_tx, clipboard_events) =
            tokio::sync::mpsc::unbounded_channel::<ClipboardEvent>();
        let local_clipboard_text: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        connector.attach_static_channel(ironrdp::cliprdr::CliprdrClient::new(Box::new(
            BridgeClipboardBackend::new(clipboard_tx, local_clipboard_text.clone()),
        )));
        // capabilities 到达即就绪；v1 不利用 caps 里的最大面积限制。
        let display_control = DisplayControlClient::new(|_capabilities| Ok(Vec::new()));
        connector.attach_static_channel(DrdynvcClient::new().with_dynamic_channel(display_control));

        let mut framed = ironrdp_tokio::TokioFramed::new(stream);
        let should_upgrade = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                ironrdp_tokio::connect_begin(&mut framed, &mut connector),
            )
            .await
            .map_err(|_| connect_timeout_error(&target, &host, port, "security negotiation"))?
            .map_err(|error| map_connector_error(&error, has_password, &target, &host, port))
        })
        .await?;

        let (stream, leftover) = framed.into_inner();
        let (tls_stream, cert) = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                ironrdp_tls::upgrade(stream, &host),
            )
            .await
            .map_err(|_| connect_timeout_error(&target, &host, port, "TLS upgrade"))?
            .map_err(|error| {
                ConnectionError::with_args(
                    "rdp_tls_failed",
                    format!("target={target}; TLS upgrade failed: {error}"),
                    serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
                    true,
                )
            })
        })
        .await?;
        // CredSSP 公钥校验需要服务器 TLS 证书公钥；取不到时传空，NTLM 降级仍能工作。
        let server_public_key = ironrdp_tls::extract_tls_server_public_key(&cert)
            .map(|key| key.to_vec())
            .unwrap_or_default();
        let mut framed = ironrdp_tokio::TokioFramed::new_with_leftover(tls_stream, leftover);
        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);

        let mut network_client = UnsupportedNetworkClient;
        let connection_result = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                ironrdp_tokio::connect_finalize(
                    upgraded,
                    connector,
                    &mut framed,
                    &mut network_client,
                    ServerName::new(host.clone()),
                    server_public_key,
                    None,
                ),
            )
            .await
            .map_err(|_| connect_timeout_error(&target, &host, port, "connection finalization"))?
            .map_err(|error| map_connector_error(&error, has_password, &target, &host, port))
        })
        .await?;

        let bridge = bind_loopback_bridge("rdp")
            .await
            .map_err(|error| bridge_failed_error(&error))?;

        let bridge_info = RdpBridgeInfo {
            url: bridge.url.clone(),
            width: u32::from(connection_result.desktop_size.width),
            height: u32::from(connection_result.desktop_size.height),
        };

        ensure_open_not_cancelled(&request)?;
        ensure_open_current(state, &request)?;
        let open_context = request.session_open_context(state);
        let session_id = spawn_bound_session(
            app,
            state,
            BoundSessionOptions {
                session_prefix: "rdp",
                connection_id: open_context.connection_id,
                transport: Box::new(RdpSessionTransport {
                    listener: bridge.listener,
                    token: bridge.token,
                    framed,
                    connection_result,
                    clipboard_events,
                    local_clipboard_text,
                }),
                capabilities: ConnectionCapabilities::rdp(),
                codec: open_context.codec,
                initial_data: None,
                startup_auth: None,
                resources: TerminalSessionResources::default(),
                replay_line_limit: open_context.replay_line_limit,
            },
        );
        crate::logging::event("terminal.rdp", "rdp.open.success")
            .field("target", &target)
            .field("session_id", &session_id)
            .field("clipboard_sync", clipboard_sync)
            .field("resize_session", resize_session)
            .info();
        Ok(ConnectionOpenResult::connected_rdp(session_id, bridge_info))
    }
}

fn build_connector_config(
    request: &ResolvedConnection,
    username: String,
    password: String,
) -> Config {
    Config {
        desktop_size: DesktopSize {
            width: INITIAL_DESKTOP_WIDTH,
            height: INITIAL_DESKTOP_HEIGHT,
        },
        desktop_scale_factor: 100,
        // TLS（图形登录）与 CredSSP（NLA）都声明，服务器选其一；只声明 NLA 会连不上
        // 未启用 NLA 的旧服务器。
        enable_tls: true,
        enable_credssp: true,
        credentials: Credentials::UsernamePassword { username, password },
        domain: request
            .rdp_domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        autologon: true,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0x0000_0409, // US；扫描码与布局无关
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        bitmap: None,
        client_build: 0,
        client_name: "xterm".to_owned(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
        #[cfg(windows)]
        platform: MajorPlatformType::WINDOWS,
        #[cfg(target_os = "macos")]
        platform: MajorPlatformType::MACINTOSH,
        #[cfg(all(unix, not(target_os = "macos")))]
        platform: MajorPlatformType::UNIX,
        // 客户端软件渲染指针：光标合成进帧缓冲，前端无需光标通道。
        enable_server_pointer: true,
        pointer_software_rendering: true,
        request_data: None,
        enable_audio_playback: false,
        compression_type: Some(CompressionType::Rdp61),
        multitransport_flags: None,
        performance_flags: PerformanceFlags::default(),
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    }
}

/// Resolves the RDP password: an inline password (auth retry) wins over the
/// saved credential. Returns `None` when no password is available; servers
/// without NLA still connect (图形登录), NLA 服务器会以 rdp_auth_required 失败。
fn resolve_rdp_password(
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
    let secret = credential_secret_by_id(state, credential_id).map_err(|error| {
        ConnectionError::with_args(
            "rdp_credential_failed",
            error.clone(),
            serde_json::json!({ "detail": error }),
            false,
        )
    })?;
    let Some(secret) = secret else {
        return Err(ConnectionError::with_args(
            "rdp_credential_failed",
            format!("selected credential '{credential_id}' does not exist"),
            serde_json::json!({ "credentialId": credential_id }),
            false,
        ));
    };
    if secret.cred_type() != "password" {
        return Err(ConnectionError::with_args(
            "rdp_credential_failed",
            format!("saved credential '{credential_id}' cannot be used for RDP authentication"),
            serde_json::json!({ "credentialId": credential_id }),
            false,
        ));
    }
    let password = secret
        .password()
        .ok_or_else(|| {
            ConnectionError::with_args(
                "rdp_credential_failed",
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

/// 把 IronRDP connector 错误映射为前端 i18n 错误码。区分认证失败的关键：
/// CredSSP/AccessDenied 说明凭据被服务器拒绝；此时若本地没有可用密码，
/// 说明服务器要求 NLA 而我们没有凭据可提交 → rdp_auth_required。
fn map_connector_error(
    error: &ConnectorError,
    has_password: bool,
    target: &str,
    host: &str,
    port: u16,
) -> ConnectionError {
    let detail = format!("target={target}; {error}");
    let args = serde_json::json!({ "host": host, "port": port, "detail": error.to_string() });
    match error.kind() {
        ConnectorErrorKind::Credssp(_) | ConnectorErrorKind::AccessDenied => {
            if has_password {
                ConnectionError::with_args("rdp_auth_failed", detail, args, false)
            } else {
                ConnectionError::with_args("rdp_auth_required", detail, args, false)
            }
        }
        ConnectorErrorKind::Negotiation(failure)
            if failure.code() == ironrdp::pdu::nego::FailureCode::HYBRID_REQUIRED_BY_SERVER
                && !has_password =>
        {
            ConnectionError::with_args("rdp_auth_required", detail, args, false)
        }
        _ => ConnectionError::with_args("rdp_connect_failed", detail, args, true),
    }
}

fn connect_timeout_error(target: &str, host: &str, port: u16, phase: &str) -> ConnectionError {
    ConnectionError::with_args(
        "rdp_connect_timeout",
        format!("target={target}; {phase} timeout"),
        serde_json::json!({ "host": host, "port": port }),
        true,
    )
}

fn connect_failed_error(
    target: &str,
    host: &str,
    port: u16,
    error: &std::io::Error,
) -> ConnectionError {
    ConnectionError::with_args(
        "rdp_connect_failed",
        format!("target={target}; {error}"),
        serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
        true,
    )
}

fn bridge_failed_error(error: &std::io::Error) -> ConnectionError {
    ConnectionError::with_args(
        "rdp_bridge_failed",
        format!("failed to start RDP bridge: {error}"),
        serde_json::json!({ "detail": error.to_string() }),
        true,
    )
}

/// CredSSP 的 Kerberos 路径需要 KDC proxy 网络客户端；v1 只支持 NTLM，
/// 提供一个直接报错的 stub（走到这里说明服务器只接受 Kerberos）。
struct UnsupportedNetworkClient;

impl ironrdp_tokio::NetworkClient for UnsupportedNetworkClient {
    async fn send(
        &mut self,
        _request: &ironrdp::connector::sspi::generator::NetworkRequest,
    ) -> ironrdp::connector::ConnectorResult<Vec<u8>> {
        Err(ConnectorError::new(
            "rdp credssp",
            ConnectorErrorKind::Reason(
                "Kerberos KDC proxy requests are not supported (NTLM only)".to_string(),
            ),
        ))
    }
}
