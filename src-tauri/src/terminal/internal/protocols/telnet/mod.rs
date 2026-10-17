use std::time::Duration;

use bytes::BytesMut;
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::{
    state::AppState,
    terminal::{
        domain::ProtocolKind,
        internal::{
            core::{
                ConnectionError, ConnectionOpenResult, ConnectionResult, ResolvedConnection,
                SessionTransportRuntime, SessionWorkerEvent, TelnetConfig, TelnetEnvVarKind,
                TerminalSessionResources, TerminalSize, TransportCommand, CONNECT_TIMEOUT_MS,
            },
            startup_auth::resolve_startup_password_auth,
            telnet_transport::spawn_telnet_transport_actor,
            terminal::{spawn_bound_session, BoundSessionOptions},
            transport_events::write_all_transport,
            util::required,
            util::{cancelable_open, ensure_open_current, ensure_open_not_cancelled},
        },
    },
};

mod engine;

use engine::{
    EngineEvent, TelnetEngine, BINARY, COMPRESS2, DO, ECHO, ENVIRON, LINEMODE, LINEMODE_MODE,
    LINEMODE_SLC, MODE_ACK, MODE_EDIT, MODE_LIT_ECHO, MODE_SOFT_TAB, MODE_TRAPSIG, NAWS,
    NEW_ENVIRON, SGA, SLC_CANTCHANGE, SLC_EC, SLC_VALUE, STATUS, TTYPE, WILL,
};

const ENV_IS: u8 = 0;
const ENV_SEND: u8 = 1;
const ENV_VAR: u8 = 0;
const ENV_VALUE: u8 = 1;
const ENV_ESC: u8 = 2;
const ENV_USERVAR: u8 = 3;
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;
const STATUS_SEND: u8 = 1;
const LOCAL_STARTUP_OPTIONS: [u8; 6] = [BINARY, SGA, TTYPE, NAWS, NEW_ENVIRON, LINEMODE];
const REMOTE_STARTUP_OPTIONS: [u8; 4] = [BINARY, ECHO, SGA, COMPRESS2];

pub(super) struct TelnetRuntime {
    reader: ReadHalf<tokio::net::TcpStream>,
    writer: WriteHalf<tokio::net::TcpStream>,
    state: TelnetState,
}

pub(super) struct TelnetSessionTransport {
    pub(super) runtime: Box<TelnetRuntime>,
    pub(super) initial_size: TerminalSize,
}

impl SessionTransportRuntime for TelnetSessionTransport {
    fn initial_size(&self) -> Option<TerminalSize> {
        Some(self.initial_size)
    }

    fn spawn(
        self: Box<Self>,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
        event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    ) {
        spawn_telnet_transport_actor(session_id, self.runtime, rx, event_tx);
    }
}

struct TelnetReceiveOutcome {
    data: Vec<u8>,
    writes: BytesMut,
    error: Option<String>,
    transport_ready: bool,
}

/// EOF is a lifecycle signal, not a protocol failure: before the transport is
/// ready it means negotiation never completed, afterwards it is a normal
/// close. Keeping it typed avoids matching on error message text at the
/// transport layer.
pub(super) enum TelnetReadError {
    RemoteClosed,
    Failed(String),
}

struct TelnetState {
    engine: TelnetEngine,
    terminal_types: Vec<String>,
    terminal_type_index: usize,
    terminal_type_exhausted: bool,
    env_vars: Vec<(TelnetEnvVarKind, String, String)>,
    cols: u16,
    rows: u16,
    pending_writes: BytesMut,
}

pub(crate) struct TelnetConnectionFactory;

impl TelnetConnectionFactory {
    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ResolvedConnection,
    ) -> ConnectionResult<ConnectionOpenResult> {
        let host = required(request.host.as_deref(), "host")
            .map_err(|error| {
                ConnectionError::with_args(
                    "telnet_host_required",
                    error.clone(),
                    serde_json::json!({ "detail": error }),
                    false,
                )
            })?
            .to_string();
        let port = request.port.unwrap_or(23);
        log::info!(target: "telnet.runtime", "opening async telnet connection to {host}:{port}");
        let stream = cancelable_open(&request, async {
            tokio::time::timeout(
                Duration::from_millis(CONNECT_TIMEOUT_MS),
                tokio::net::TcpStream::connect((host.as_str(), port)),
            )
            .await
            .map_err(|_| {
                ConnectionError::with_args(
                    "telnet_connect_timeout",
                    format!("target={host}:{port}; connect timeout"),
                    serde_json::json!({ "host": host, "port": port }),
                    true,
                )
            })?
            .map_err(|error| {
                ConnectionError::with_args(
                    "telnet_connect_failed",
                    format!("target={host}:{port}; {error}"),
                    serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
                    true,
                )
            })
        })
        .await?;
        stream.set_nodelay(true).map_err(|error| {
            ConnectionError::with_args(
                "telnet_stream_setup_failed",
                error.to_string(),
                serde_json::json!({ "detail": error.to_string() }),
                true,
            )
        })?;
        configure_telnet_tcp_keepalive(&stream);

        let encoding = normalize_telnet_charset(request.encoding.as_deref());
        let open_context = request.session_open_context();
        let config = TelnetConfig {
            terminal_type: normalize_terminal_type(request.terminal_type.as_deref()),
            cols: request.cols.unwrap_or(80).clamp(1, u16::MAX as u32) as u16,
            rows: request.rows.unwrap_or(24).clamp(1, u16::MAX as u32) as u16,
            env_vars: telnet_env_var_list(request.terminal_type.as_deref(), encoding.as_deref()),
            backspace_sends: request.backspace_sends.clone(),
        };
        let initial_size = TerminalSize {
            cols: config.cols as u32,
            rows: config.rows as u32,
        };
        let mut runtime = TelnetRuntime::new(stream, config);
        cancelable_open(&request, async {
            runtime.flush_startup().await.map_err(|error| {
                ConnectionError::with_args(
                    "telnet_startup_write_failed",
                    format!("target={host}:{port}; {error}"),
                    serde_json::json!({ "host": host, "port": port, "detail": error.to_string() }),
                    false,
                )
            })
        })
        .await?;
        let startup_auth =
            resolve_startup_password_auth(state, &request, "telnet_startup_auth_failed")?;

        ensure_open_not_cancelled(&request)?;
        ensure_open_current(state, &request)?;
        let session_id = spawn_bound_session(
            app,
            state,
            BoundSessionOptions {
                session_prefix: "telnet",
                connection_id: open_context.connection_id,
                transport: Box::new(TelnetSessionTransport {
                    runtime: Box::new(runtime),
                    initial_size,
                }),
                capabilities: crate::terminal::domain::ConnectionCapabilities::telnet(),
                codec: open_context.codec,
                initial_data: None,
                startup_auth,
                resources: TerminalSessionResources::default(),
            },
        );
        Ok(ConnectionOpenResult::connected_shell(
            session_id,
            ProtocolKind::Telnet,
            open_context.encoding_label,
        ))
    }
}

impl TelnetRuntime {
    pub(super) fn new(stream: tokio::net::TcpStream, config: TelnetConfig) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self {
            reader,
            writer,
            state: TelnetState::new(config),
        }
    }

    async fn flush_startup(&mut self) -> Result<(), String> {
        if self.state.pending_writes.is_empty() {
            return Ok(());
        }
        let bytes = self.state.pending_writes.split().freeze();
        write_telnet_socket(&mut self.writer, &bytes).await
    }

    /// Reads from the socket and feeds the bytes through the protocol engine.
    /// Decoded application data is returned as an owned buffer (moved out of
    /// the engine's event without an extra copy); `buffer` is only the socket
    /// read target.
    pub(super) async fn read_into_with_negotiation(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(Vec<u8>, bool), TelnetReadError> {
        if buffer.is_empty() {
            return Err(TelnetReadError::Failed(
                "telnet read buffer must not be empty".to_string(),
            ));
        }
        let size = match self.reader.read(buffer).await {
            Ok(0) => return Err(TelnetReadError::RemoteClosed),
            Ok(size) => size,
            Err(error) => {
                return Err(TelnetReadError::Failed(format!(
                    "failed to read telnet data: {error}"
                )));
            }
        };
        self.process_socket_bytes(&buffer[..size])
            .await
            .map_err(TelnetReadError::Failed)
    }

    pub(super) async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let events = self.state.engine.send_terminal_input(bytes);
        let writes = collect_protocol_writes(events);
        write_telnet_socket(&mut self.writer, &writes).await
    }

    pub(super) fn set_backspace_mode(&mut self, mode: Option<&str>) {
        self.state.engine.configure_backspace_mode(mode);
    }

    pub(super) fn erase_probe_deadline(&self) -> Option<std::time::Instant> {
        self.state.engine.erase_probe_deadline()
    }

    pub(super) async fn flush_erase_probe_retry(&mut self) -> Result<(), String> {
        if let Some(byte) = self.state.engine.poll_erase_probe() {
            write_telnet_socket(&mut self.writer, &[byte]).await?;
        }
        Ok(())
    }

    pub(super) async fn resize(&mut self, cols: u32, rows: u32) -> Result<(), String> {
        self.state.cols = cols.clamp(1, u16::MAX as u32) as u16;
        self.state.rows = rows.clamp(1, u16::MAX as u32) as u16;
        if self.state.engine.local_enabled(NAWS) {
            let frame = self.state.naws_events();
            write_telnet_socket(&mut self.writer, &frame).await?;
        }
        Ok(())
    }

    pub(super) async fn close(&mut self) -> Result<(), String> {
        let _ = self.writer.shutdown().await;
        Ok(())
    }

    async fn process_socket_bytes(&mut self, raw: &[u8]) -> Result<(Vec<u8>, bool), String> {
        let outcome = self.state.receive(raw);
        if !outcome.writes.is_empty() {
            write_telnet_socket(&mut self.writer, &outcome.writes).await?;
        }
        if let Some(error) = outcome.error {
            return Err(error);
        }
        Ok((outcome.data, outcome.transport_ready))
    }
}

impl TelnetState {
    fn new(config: TelnetConfig) -> Self {
        let backspace_sends = config.backspace_sends.clone();
        let mut state = Self {
            engine: TelnetEngine::new(),
            terminal_types: terminal_type_cycle(&config.terminal_type),
            terminal_type_index: 0,
            terminal_type_exhausted: false,
            env_vars: config.env_vars,
            cols: config.cols,
            rows: config.rows,
            pending_writes: BytesMut::with_capacity(64),
        };
        state
            .engine
            .configure_backspace_mode(backspace_sends.as_deref());
        for option in LOCAL_STARTUP_OPTIONS {
            let events = state.engine.negotiate(WILL, option);
            state
                .pending_writes
                .extend_from_slice(&collect_protocol_writes(events));
        }
        for option in REMOTE_STARTUP_OPTIONS {
            let events = state.engine.negotiate(DO, option);
            state
                .pending_writes
                .extend_from_slice(&collect_protocol_writes(events));
        }
        state
    }

    fn receive(&mut self, raw: &[u8]) -> TelnetReceiveOutcome {
        let mut writes = BytesMut::with_capacity(64);
        let mut received: Option<Vec<u8>> = None;
        let mut error = None;
        let mut transport_ready = false;
        for event in self.engine.receive(raw) {
            if matches!(event, EngineEvent::Negotiation { .. }) {
                transport_ready = true;
            }
            match event {
                EngineEvent::Data(data) => {
                    if !data.is_empty() {
                        // Telnet option negotiation is optional. A server that
                        // starts with application data is already usable even
                        // if it never acknowledges our startup options.
                        transport_ready = true;
                        if let Some(retry) = self.engine.observe_output(&data) {
                            writes.extend_from_slice(&[retry]);
                        }
                        // A read usually yields exactly one data segment (IAC is
                        // rare in payloads), so move the first segment and only
                        // copy when IAC-separated segments must coalesce.
                        match &mut received {
                            None => received = Some(data),
                            Some(acc) => acc.extend_from_slice(&data),
                        }
                    }
                }
                EngineEvent::Send(data) => writes.extend_from_slice(&data),
                EngineEvent::Negotiation {
                    command: DO,
                    option: NAWS,
                } => writes.extend_from_slice(&self.naws_events()),
                EngineEvent::Negotiation {
                    command: DO,
                    option: STATUS,
                } => writes.extend_from_slice(&self.status_events()),
                EngineEvent::Negotiation {
                    command: DO,
                    option: LINEMODE,
                } => writes.extend_from_slice(&self.linemode_slc_query_events()),
                EngineEvent::Subnegotiation {
                    option: TTYPE,
                    data,
                } => {
                    if data.first() == Some(&TTYPE_SEND) {
                        writes.extend_from_slice(&self.ttype_events());
                    }
                }
                EngineEvent::Subnegotiation {
                    option: option @ (ENVIRON | NEW_ENVIRON),
                    data,
                } => {
                    if data.first() == Some(&ENV_SEND) {
                        writes.extend_from_slice(&self.new_environ_events(option, &data));
                    }
                }
                EngineEvent::Subnegotiation {
                    option: STATUS,
                    data,
                } => {
                    if data.first() == Some(&STATUS_SEND) {
                        writes.extend_from_slice(&self.status_events());
                    }
                }
                EngineEvent::Subnegotiation {
                    option: LINEMODE,
                    data,
                } => match data.first() {
                    Some(&LINEMODE_MODE) => {
                        if let Some(ack) = linemode_mode_ack(&data) {
                            writes.extend_from_slice(&collect_protocol_writes(
                                self.engine.subnegotiation(LINEMODE, &ack),
                            ));
                        }
                    }
                    _ => {
                        if let Some(erase) = parse_linemode_erase_character(&data) {
                            self.engine.set_remote_erase_character(erase);
                            log::debug!(
                                target: "telnet.runtime",
                                "server SLC_EC erase character: 0x{erase:02x}"
                            );
                        }
                    }
                },
                EngineEvent::Warning(detail) => {
                    log::warn!(target: "telnet.runtime", "protocol warning: {detail}")
                }
                EngineEvent::Error(detail) => {
                    // The first error is the root cause; later ones from the
                    // same read are cascade noise and only worth a log line.
                    if error.is_none() {
                        error = Some(detail);
                    } else {
                        log::warn!(target: "telnet.runtime", "superseded protocol error: {detail}");
                    }
                }
                EngineEvent::Iac(command) => {
                    log::trace!(target: "telnet.runtime", "telnet command ignored: {command}");
                }
                EngineEvent::Negotiation { .. } | EngineEvent::Subnegotiation { .. } => {}
            }
        }
        TelnetReceiveOutcome {
            data: received.unwrap_or_default(),
            writes,
            error,
            transport_ready,
        }
    }

    fn naws_events(&mut self) -> Vec<u8> {
        let mut payload = [0_u8; 4];
        payload[..2].copy_from_slice(&self.cols.to_be_bytes());
        payload[2..].copy_from_slice(&self.rows.to_be_bytes());
        collect_protocol_writes(self.engine.subnegotiation(NAWS, &payload))
    }

    fn ttype_events(&mut self) -> Vec<u8> {
        // `terminal_types` always holds at least the primary type and the
        // index advance below keeps it in range, so indexing cannot fail.
        let terminal_type = self.terminal_types[self.terminal_type_index].clone();
        if self.terminal_type_index + 1 < self.terminal_types.len() {
            self.terminal_type_index += 1;
        } else if self.terminal_type_exhausted {
            self.terminal_type_index = 0;
            self.terminal_type_exhausted = false;
        } else {
            self.terminal_type_exhausted = true;
        }
        let mut payload = Vec::with_capacity(terminal_type.len() + 1);
        payload.push(TTYPE_IS);
        payload.extend_from_slice(terminal_type.as_bytes());
        collect_protocol_writes(self.engine.subnegotiation(TTYPE, &payload))
    }

    fn new_environ_events(&mut self, option: u8, request: &[u8]) -> Vec<u8> {
        let requested = parse_new_environ_send(request);
        let mut payload = Vec::with_capacity(64);
        payload.push(ENV_IS);
        if requested.is_empty() {
            for (kind, name, value) in &self.env_vars {
                push_env_value(&mut payload, *kind, name, value);
            }
        } else {
            for (kind, requested_name) in requested {
                for (value_kind, name, value) in &self.env_vars {
                    if kind == *value_kind
                        && (requested_name.is_empty() || requested_name.eq_ignore_ascii_case(name))
                    {
                        push_env_value(&mut payload, *value_kind, name, value);
                    }
                }
            }
        }
        collect_protocol_writes(self.engine.subnegotiation(option, &payload))
    }

    fn status_events(&mut self) -> Vec<u8> {
        let payload = self.engine.status_payload();
        collect_protocol_writes(self.engine.subnegotiation(STATUS, &payload))
    }

    fn linemode_slc_query_events(&mut self) -> Vec<u8> {
        // RFC 1184: SLC function 0 with SLC_VALUE requests the server's
        // current special-character table. We only consume SLC_EC; local
        // line editing remains disabled so xterm.js keeps character mode.
        collect_protocol_writes(
            self.engine
                .subnegotiation(LINEMODE, &[LINEMODE_SLC, 0, SLC_VALUE, 0]),
        )
    }
}

fn collect_protocol_writes(events: Vec<EngineEvent>) -> Vec<u8> {
    let mut writes = Vec::new();
    for event in events {
        match event {
            EngineEvent::Send(data) => writes.extend_from_slice(&data),
            EngineEvent::Warning(detail) => {
                log::warn!(target: "telnet.runtime", "protocol warning: {detail}")
            }
            _ => {}
        }
    }
    writes
}

async fn write_telnet_socket(
    writer: &mut WriteHalf<tokio::net::TcpStream>,
    bytes: &[u8],
) -> Result<(), String> {
    if !bytes.is_empty() {
        write_all_transport(writer, bytes, "telnet").await?;
    }
    Ok(())
}

/// Enables TCP keepalive on the telnet socket (30s idle, 30s interval,
/// 3 retries where the platform supports configuring the retry count).
/// Best-effort: a platform or driver that rejects these options must not
/// block an otherwise healthy connection.
fn configure_telnet_tcp_keepalive(stream: &tokio::net::TcpStream) {
    let socket = socket2::SockRef::from(stream);
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(30));
    #[cfg(any(
        target_os = "android",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "ios",
        target_os = "visionos",
        target_os = "linux",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "cygwin",
        target_os = "windows",
        all(target_os = "wasi", not(target_env = "p1")),
    ))]
    let keepalive = keepalive.with_retries(3);
    if let Err(error) = socket.set_tcp_keepalive(&keepalive) {
        log::warn!(target: "telnet.runtime", "failed to configure telnet TCP keepalive: {error}");
    }
}

fn terminal_type_cycle(primary: &str) -> Vec<String> {
    let mut values = vec![primary.to_string()];
    for fallback in [
        "xterm-256color",
        "xterm",
        "xterm-color",
        "vt220",
        "vt100",
        "ansi",
    ] {
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(fallback))
        {
            values.push(fallback.to_string());
        }
    }
    values
}

fn normalize_telnet_charset(value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("auto") {
        return None;
    }
    encoding_rs::Encoding::for_label(raw.as_bytes())
        .map(|encoding| encoding.name().to_ascii_uppercase())
}

fn telnet_env_var_list(
    terminal_type: Option<&str>,
    charset: Option<&str>,
) -> Vec<(TelnetEnvVarKind, String, String)> {
    let lang = charset
        .map(|item| format!("C.{item}"))
        .unwrap_or_else(|| "C.UTF-8".to_string());
    vec![
        (
            TelnetEnvVarKind::UserDefined,
            "TERM".to_string(),
            normalize_terminal_type(terminal_type),
        ),
        (
            TelnetEnvVarKind::UserDefined,
            "LANG".to_string(),
            lang.clone(),
        ),
        (TelnetEnvVarKind::UserDefined, "LC_CTYPE".to_string(), lang),
    ]
}

fn parse_new_environ_send(payload: &[u8]) -> Vec<(TelnetEnvVarKind, String)> {
    let mut requested = Vec::new();
    let mut index = 1;
    while index < payload.len() {
        let kind = match payload[index] {
            ENV_VAR => TelnetEnvVarKind::WellKnown,
            ENV_USERVAR => TelnetEnvVarKind::UserDefined,
            _ => {
                index += 1;
                continue;
            }
        };
        index += 1;
        let mut name = Vec::new();
        while index < payload.len() {
            match payload[index] {
                ENV_VAR | ENV_USERVAR | ENV_VALUE => break,
                ENV_ESC => {
                    index += 1;
                    if let Some(byte) = payload.get(index) {
                        name.push(*byte);
                    }
                }
                byte => name.push(byte),
            }
            index += 1;
        }
        requested.push((kind, String::from_utf8_lossy(&name).into_owned()));
    }
    requested
}

fn push_env_value(output: &mut Vec<u8>, kind: TelnetEnvVarKind, name: &str, value: &str) {
    output.push(match kind {
        TelnetEnvVarKind::WellKnown => ENV_VAR,
        TelnetEnvVarKind::UserDefined => ENV_USERVAR,
    });
    push_env_escaped(output, name.as_bytes());
    output.push(ENV_VALUE);
    push_env_escaped(output, value.as_bytes());
}

fn push_env_escaped(output: &mut Vec<u8>, value: &[u8]) {
    for &byte in value {
        if matches!(byte, ENV_VAR | ENV_VALUE | ENV_ESC | ENV_USERVAR) {
            output.push(ENV_ESC);
        }
        output.push(byte);
    }
}

fn parse_linemode_erase_character(payload: &[u8]) -> Option<u8> {
    // 只解析 SLC 子命令；MODE/FORWARDMASK 等其他子命令不携带字符表。
    if payload.first() != Some(&LINEMODE_SLC) {
        return None;
    }
    let mut index = 1;
    while index + 2 < payload.len() {
        if payload[index] == SLC_EC {
            // The low two bits are the SLC level; ACK/flush flags occupy the
            // high bits. CANTCHANGE still carries the current value and is
            // therefore just as usable as VALUE. Ignore DEFAULT/NOSUPPORT,
            // which do not identify a character to send.
            let level = payload[index + 1] & 0x03;
            if matches!(level, SLC_CANTCHANGE | SLC_VALUE) {
                return Some(payload[index + 2]);
            }
        }
        index += 3;
    }
    None
}

/// RFC 1184 MODE 应答：本端不做本地行编辑，对服务器的 MODE 建议回 ACK 并清掉
/// EDIT（SOFT_TAB/LIT_ECHO 是 EDIT 的修饰位、TRAPSIG 只在本地编辑时有意义，一
/// 并清掉），让服务器继续做行编辑与回显。不答或答错都会把行编辑职责留在两端
/// 之间悬空；对方发来的 ACK 不再回应，避免来回确认。
fn linemode_mode_ack(payload: &[u8]) -> Option<[u8; 2]> {
    let mode = *payload.get(1)?;
    if mode & MODE_ACK != 0 {
        return None;
    }
    Some([
        LINEMODE_MODE,
        (mode & !(MODE_EDIT | MODE_TRAPSIG | MODE_SOFT_TAB | MODE_LIT_ECHO)) | MODE_ACK,
    ])
}

pub(super) fn normalize_terminal_type(value: Option<&str>) -> String {
    match value.unwrap_or("xterm-256color").trim() {
        "xterm" => "xterm",
        "xterm-256color" => "xterm-256color",
        "xterm-color" => "xterm-color",
        "vt100" => "vt100",
        "vt220" => "vt220",
        "ansi" => "ansi",
        _ => "xterm-256color",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::engine::{
        DONT, IAC, LINEMODE, LINEMODE_MODE, LINEMODE_SLC, MODE_ACK, SLC_EC, SLC_VALUE, STATUS_IS,
    };
    use super::*;

    fn test_config() -> TelnetConfig {
        TelnetConfig {
            terminal_type: "xterm-256color".to_string(),
            cols: 80,
            rows: 24,
            env_vars: telnet_env_var_list(Some("xterm-256color"), Some("UTF-8")),
            backspace_sends: Some("AUTO".to_string()),
        }
    }

    #[test]
    fn startup_negotiation_matches_switch_client_policy() {
        let state = TelnetState::new(test_config());
        let writes = state.pending_writes.as_ref();
        for frame in [
            [255, WILL, SGA],
            [255, WILL, TTYPE],
            [255, WILL, NAWS],
            [255, WILL, NEW_ENVIRON],
            [255, DO, ECHO],
            [255, DO, SGA],
        ] {
            assert!(writes.windows(3).any(|candidate| candidate == frame));
        }
        assert!(writes
            .windows(3)
            .any(|candidate| candidate == [255, WILL, BINARY]));
        assert!(writes
            .windows(3)
            .any(|candidate| candidate == [255, DO, BINARY]));
        assert!(writes
            .windows(3)
            .any(|candidate| candidate == [255, DO, COMPRESS2]));
        assert!(writes
            .windows(3)
            .any(|candidate| candidate == [255, WILL, LINEMODE]));
    }

    #[test]
    fn linemode_will_triggers_slc_query_and_erase_value_is_applied() {
        let mut state = TelnetState::new(test_config());
        let outcome = state.receive(&[IAC, DO, LINEMODE]);
        assert!(outcome
            .writes
            .windows(8)
            .any(|frame| { frame == [IAC, 250, LINEMODE, LINEMODE_SLC, 0, SLC_VALUE, 0, IAC] }));

        let outcome = state.receive(&[
            IAC,
            250,
            LINEMODE,
            LINEMODE_SLC,
            SLC_EC,
            SLC_VALUE,
            8,
            IAC,
            240,
        ]);
        assert!(outcome.error.is_none());
        assert_eq!(
            super::collect_protocol_writes(state.engine.send_terminal_input(b"\x7f")),
            b"\x08"
        );
    }

    #[test]
    fn linemode_mode_is_acknowledged_without_local_editing() {
        let mut state = TelnetState::new(test_config());
        // 服务器建议 EDIT|TRAPSIG：本端 ACK 且全部清掉（不做本地行编辑）。
        let outcome = state.receive(&[IAC, 250, LINEMODE, LINEMODE_MODE, 0x03, IAC, 240]);
        assert_eq!(
            outcome.writes.as_ref(),
            [IAC, 250, LINEMODE, LINEMODE_MODE, MODE_ACK, IAC, 240]
        );
        // 对方发来的 ACK 不再回应，避免来回确认。
        let outcome = state.receive(&[IAC, 250, LINEMODE, LINEMODE_MODE, MODE_ACK, IAC, 240]);
        assert!(outcome.writes.is_empty());
    }

    #[test]
    fn linemode_mode_payload_is_not_mistaken_for_an_slc_table() {
        let mut state = TelnetState::new(test_config());
        // MODE 子命令的字节布局恰好满足三元组扫描时不得产出擦除字符。
        let outcome = state.receive(&[
            IAC,
            250,
            LINEMODE,
            LINEMODE_MODE,
            SLC_EC,
            SLC_VALUE,
            IAC,
            240,
        ]);
        assert!(outcome.error.is_none());
        assert_eq!(
            super::collect_protocol_writes(state.engine.send_terminal_input(b"\x7f")),
            b"\x7f"
        );
    }

    #[test]
    fn linemode_ignores_non_value_erase_entries() {
        let mut state = TelnetState::new(test_config());
        let outcome = state.receive(&[IAC, 250, LINEMODE, LINEMODE_SLC, SLC_EC, 3, 0, IAC, 240]);
        assert!(outcome.error.is_none());
        assert_eq!(
            super::collect_protocol_writes(state.engine.send_terminal_input(b"\x7f")),
            b"\x7f"
        );
    }

    #[test]
    fn naws_is_sent_only_after_server_accepts_it() {
        let mut state = TelnetState::new(test_config());
        let outcome = state.receive(&[255, DO, NAWS]);
        assert!(outcome.transport_ready);
        assert!(outcome
            .writes
            .windows(9)
            .any(|bytes| bytes == [255, 250, NAWS, 0, 80, 0, 24, 255, 240]));

        let duplicate = state.receive(&[255, DO, NAWS]);
        assert!(!duplicate.transport_ready);
    }

    #[test]
    fn ttype_cycle_repeats_last_then_restarts() {
        let mut state = TelnetState::new(test_config());
        let _ = state.receive(&[255, DO, TTYPE]);
        let request = [255, 250, TTYPE, TTYPE_SEND, 255, 240];
        let mut names = Vec::new();
        for _ in 0..8 {
            let outcome = state.receive(&request);
            let start = outcome
                .writes
                .windows(4)
                .position(|w| w == [255, 250, TTYPE, TTYPE_IS])
                .unwrap();
            names.push(
                String::from_utf8_lossy(&outcome.writes[start + 4..outcome.writes.len() - 2])
                    .into_owned(),
            );
        }
        assert_eq!(names[5], "ansi");
        assert_eq!(names[6], "ansi");
        assert_eq!(names[7], "xterm-256color");
    }

    #[test]
    fn new_environ_preserves_request_order_and_empty_type_means_all() {
        let mut state = TelnetState::new(test_config());
        let _ = state.receive(&[255, DO, NEW_ENVIRON]);
        let outcome = state.receive(&[
            255,
            250,
            NEW_ENVIRON,
            ENV_SEND,
            ENV_USERVAR,
            b'L',
            b'A',
            b'N',
            b'G',
            ENV_USERVAR,
            255,
            240,
        ]);
        let body = &outcome.writes[3..outcome.writes.len() - 2];
        let text = String::from_utf8_lossy(body);
        assert!(text.starts_with(char::from(ENV_IS)));
        assert!(text.find("LANG").unwrap() < text.find("TERM").unwrap());
        assert!(text.contains("LC_CTYPE"));
    }

    #[test]
    fn legacy_environ_is_supported_when_requested_by_the_server() {
        let mut state = TelnetState::new(test_config());
        let outcome = state.receive(&[255, DO, ENVIRON]);
        assert!(outcome.transport_ready);
        assert!(outcome
            .writes
            .windows(3)
            .any(|frame| frame == [255, WILL, ENVIRON]));

        let outcome = state.receive(&[255, 250, ENVIRON, ENV_SEND, 255, 240]);
        assert!(outcome
            .writes
            .windows(4)
            .any(|frame| frame == [255, 250, ENVIRON, ENV_IS]));
    }

    #[test]
    fn status_option_reports_current_negotiation_state() {
        let mut state = TelnetState::new(test_config());
        let outcome = state.receive(&[255, DO, STATUS]);
        assert!(outcome.transport_ready);
        assert!(outcome
            .writes
            .windows(3)
            .any(|frame| frame == [255, WILL, STATUS]));
        assert!(outcome
            .writes
            .windows(4)
            .any(|frame| frame == [255, 250, STATUS, STATUS_IS]));

        let outcome = state.receive(&[255, 250, STATUS, STATUS_SEND, 255, 240]);
        assert!(outcome
            .writes
            .windows(4)
            .any(|frame| frame == [255, 250, STATUS, STATUS_IS]));
    }

    #[test]
    fn unsupported_eor_charset_and_mccp_are_rejected() {
        let mut state = TelnetState::new(test_config());
        for option in [25, 42, 85, 87] {
            let outcome = state.receive(&[255, WILL, option]);
            assert!(!outcome.transport_ready);
            assert_eq!(outcome.writes.as_ref(), [255, DONT, option]);
        }
    }

    #[test]
    fn application_data_marks_plain_telnet_transport_ready() {
        let mut state = TelnetState::new(test_config());

        let outcome = state.receive(b"login: ");

        assert!(outcome.transport_ready);
        assert_eq!(outcome.data, b"login: ");
    }

    #[tokio::test]
    async fn startup_sends_iac_without_waiting_for_remote_data() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (client, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        let mut server = accepted.unwrap().0;
        let mut runtime = TelnetRuntime::new(client.unwrap(), test_config());

        tokio::time::timeout(Duration::from_millis(100), runtime.flush_startup())
            .await
            .expect("startup write must not wait for a remote response")
            .unwrap();

        let mut startup = [0_u8; 64];
        let size = tokio::time::timeout(Duration::from_millis(100), server.read(&mut startup))
            .await
            .unwrap()
            .unwrap();
        assert!(startup[..size].windows(2).any(|bytes| bytes[0] == 255));
    }

    /// 还原设备侧真实场景：服务器回显可打印字符、对 DEL 完全沉默、对 BS 回显
    /// BS SP BS。AUTO 模式必须先原样发出 DEL，探测窗沉默后透明补发 BS，并在
    /// 看到擦除回显后把后续 DEL 按键改写为 BS。
    #[tokio::test]
    async fn auto_backspace_learns_bs_from_echo_probing() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (client, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        let mut server = accepted.unwrap().0;
        let mut runtime = TelnetRuntime::new(client.unwrap(), test_config());
        runtime.flush_startup().await.unwrap();

        let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
        let server_task = tokio::spawn(async move {
            let mut buffer = [0_u8; 256];
            let mut iac_skip = 0_usize;
            loop {
                let size = server.read(&mut buffer).await.unwrap();
                if size == 0 {
                    return;
                }
                for &byte in &buffer[..size] {
                    if iac_skip > 0 {
                        iac_skip -= 1;
                        continue;
                    }
                    if byte == 255 {
                        iac_skip = 2;
                        continue;
                    }
                    seen_tx.send(byte).unwrap();
                    match byte {
                        0x7f => {}
                        0x08 => server.write_all(b"\x08 \x08").await.unwrap(),
                        byte if byte.is_ascii_graphic() => {
                            server.write_all(&[byte]).await.unwrap();
                        }
                        _ => {}
                    }
                }
            }
        });

        let mut buffer = [0_u8; 256];

        async fn read_some(runtime: &mut TelnetRuntime, buffer: &mut [u8]) {
            match tokio::time::timeout(
                Duration::from_millis(2_000),
                runtime.read_into_with_negotiation(buffer),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(_)) => panic!("read failed"),
                Err(_) => panic!("read timed out"),
            }
        }

        // 打字建立回显活性。
        runtime.write(b"a").await.unwrap();
        read_some(&mut runtime, &mut buffer).await;
        assert_eq!(seen_rx.recv().await, Some(b'a'));

        // 第一次退格：DEL 原样发出，设备沉默。
        runtime.write(b"\x7f").await.unwrap();
        assert_eq!(seen_rx.recv().await, Some(0x7f));
        let deadline = runtime.erase_probe_deadline().expect("probe must be armed");
        tokio::time::sleep_until(deadline.into()).await;
        // 探测窗到期：回显活跃下的沉默 → 透明补发 BS，设备回显擦除序列。
        runtime.flush_erase_probe_retry().await.unwrap();
        assert_eq!(seen_rx.recv().await, Some(0x08));
        read_some(&mut runtime, &mut buffer).await;

        // 学习完成：后续 DEL 按键改写为 BS。
        runtime.write(b"\x7f").await.unwrap();
        assert_eq!(seen_rx.recv().await, Some(0x08));
        server_task.abort();
    }
}
