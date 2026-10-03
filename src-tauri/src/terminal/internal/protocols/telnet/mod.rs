use std::time::Duration;

use bytes::{Buf, BytesMut};
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
                SESSION_BUFFER_SIZE,
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
    EngineEvent, TelnetEngine, BINARY, COMPRESS2, DO, ECHO, ENVIRON, NAWS, NEW_ENVIRON, SGA,
    STATUS, TTYPE, WILL,
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
const LOCAL_STARTUP_OPTIONS: [u8; 5] = [BINARY, SGA, TTYPE, NAWS, NEW_ENVIRON];
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
    pending_data: BytesMut,
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
        let open_context = request.session_open_context(state);
        let config = TelnetConfig {
            terminal_type: normalize_terminal_type(request.terminal_type.as_deref()),
            cols: request.cols.unwrap_or(80).clamp(1, u16::MAX as u32) as u16,
            rows: request.rows.unwrap_or(24).clamp(1, u16::MAX as u32) as u16,
            env_vars: telnet_env_var_list(request.terminal_type.as_deref(), encoding.as_deref()),
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
                replay_line_limit: open_context.replay_line_limit,
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

    /// Reads from the socket, feeds the bytes through the protocol engine and
    /// drains whatever application data decoded into `buffer`. The socket read
    /// targets `buffer` directly; decoded surplus (e.g. MCCP2 output larger
    /// than one read) stays in `pending_data` for the next call.
    pub(super) async fn read_into_with_negotiation(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(usize, bool), TelnetReadError> {
        if self.state.pending_data.has_remaining() {
            return Ok((drain_pending_data(&mut self.state, buffer), false));
        }
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
        let transport_ready = self
            .process_socket_bytes(&buffer[..size])
            .await
            .map_err(TelnetReadError::Failed)?;
        Ok((drain_pending_data(&mut self.state, buffer), transport_ready))
    }

    pub(super) async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let events = self.state.engine.send_terminal_input(bytes);
        let writes = collect_protocol_writes(events);
        write_telnet_socket(&mut self.writer, &writes).await
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

    async fn process_socket_bytes(&mut self, raw: &[u8]) -> Result<bool, String> {
        let outcome = self.state.receive(raw);
        if !outcome.writes.is_empty() {
            write_telnet_socket(&mut self.writer, &outcome.writes).await?;
        }
        if let Some(error) = outcome.error {
            return Err(error);
        }
        Ok(outcome.transport_ready)
    }
}

impl TelnetState {
    fn new(config: TelnetConfig) -> Self {
        let mut state = Self {
            engine: TelnetEngine::new(),
            terminal_types: terminal_type_cycle(&config.terminal_type),
            terminal_type_index: 0,
            terminal_type_exhausted: false,
            env_vars: config.env_vars,
            cols: config.cols,
            rows: config.rows,
            pending_data: BytesMut::with_capacity(SESSION_BUFFER_SIZE),
            pending_writes: BytesMut::with_capacity(64),
        };
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
                        self.pending_data.extend_from_slice(&data);
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

fn drain_pending_data(state: &mut TelnetState, output: &mut [u8]) -> usize {
    let size = output.len().min(state.pending_data.len());
    if size > 0 {
        output[..size].copy_from_slice(&state.pending_data.split_to(size));
    }
    size
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
    use super::engine::{DONT, STATUS_IS};
    use super::*;

    fn test_config() -> TelnetConfig {
        TelnetConfig {
            terminal_type: "xterm-256color".to_string(),
            cols: 80,
            rows: 24,
            env_vars: telnet_env_var_list(Some("xterm-256color"), Some("UTF-8")),
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
        assert_eq!(state.pending_data.as_ref(), b"login: ");
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
}
