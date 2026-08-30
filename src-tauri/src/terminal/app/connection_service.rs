use tauri::AppHandle;

use crate::{
    logging,
    state::{AppState, ConnectionStatus},
    terminal::{
        api::dto::{
            ConnectionAuthenticateCommand, ConnectionHostKeyChallengePayload,
            ConnectionOpenResponse,
        },
        domain::{ProtocolKind, TerminalApiError},
        events::emit_connection_host_key_challenge,
        internal::{
            resolve_connection_request, ConnectionError, ConnectionOpenRequest,
            ConnectionOpenResult, ResolvedConnection, SshRuntimeMetricsRequest,
        },
        protocol::protocol_registry,
        session_service,
    },
};

#[derive(Clone, Copy, Default)]
pub(crate) struct ConnectionApplicationService;

impl ConnectionApplicationService {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ConnectionOpenRequest,
    ) -> Result<ConnectionOpenResponse, TerminalApiError> {
        let log = logging::event("terminal.connection_service", "connection.open")
            .field("connection_id", &request.connection_id)
            .maybe_field("open_request_id", request.open_request_id.clone())
            .maybe_field("cols", request.cols)
            .maybe_field("rows", request.rows)
            .maybe_field("terminal_type", request.terminal_type.clone());
        log.clone().info();

        let resolved =
            resolve_connection_request(state, request).map_err(TerminalApiError::from)?;
        let open_request_id = resolved.open_request_id.as_deref().unwrap_or(&resolved.id);
        let open_scope = state.begin_connection_open(open_request_id);
        let resolved = resolved.with_open_scope(open_scope);
        self.connect(app, state, resolved, log).await
    }

    /// Shared connection execution.  Once the `ResolvedConnection` is
    /// available (from store lookup or direct construction), the protocol
    /// dispatch, state emission, adapter call, and outcome handling are
    /// the same regardless of how the connection was initiated.
    async fn connect(
        &self,
        app: AppHandle,
        state: &AppState,
        resolved: ResolvedConnection,
        log: crate::logging::LogEvent,
    ) -> Result<ConnectionOpenResponse, TerminalApiError> {
        let protocol = resolved.protocol;
        let driver = protocol_registry().require_driver(protocol);
        driver.validate(&resolved).map_err(TerminalApiError::from)?;
        let protocol = driver.kind();
        let log = log
            .field("protocol", protocol.as_str())
            .maybe_field("host", resolved.host.clone())
            .maybe_field("port", resolved.port)
            .maybe_field("user", resolved.user.clone());

        let open_scope = resolved.open_scope.clone();
        let open_request_id = resolved.open_request_id.as_deref().unwrap_or(&resolved.id);
        if !open_scope
            .as_ref()
            .is_some_and(|scope| state.connection_open_matches(open_request_id, scope))
        {
            if let Some(scope) = open_scope.as_ref() {
                state.finish_connection_open(open_request_id, scope);
            }
            return Err(TerminalApiError::invalid("connection open was superseded"));
        }
        set_connection_state(
            state,
            &resolved.id,
            protocol,
            ConnectionStatus::Connecting,
            None,
        );

        match driver.open(app.clone(), state, resolved.clone()).await {
            Ok(ConnectionOpenResult::Connected {
                session_id,
                serial_port,
                baud_rate,
                serial_scores,
                ..
            }) => {
                if !open_scope
                    .as_ref()
                    .is_some_and(|scope| state.connection_open_matches(open_request_id, scope))
                {
                    let _ = session_service().close(state, &session_id);
                    if let Some(scope) = open_scope.as_ref() {
                        state.finish_connection_open(open_request_id, scope);
                    }
                    log.clone()
                        .field("session_id", &session_id)
                        .field("result", "stale_connected_discarded")
                        .warn();
                    return Err(TerminalApiError::invalid("connection open was superseded"));
                }
                log.clone()
                    .field("session_id", &session_id)
                    .maybe_field("serial_port", serial_port.clone())
                    .maybe_field("baud_rate", baud_rate)
                    .info();
                set_connection_state(
                    state,
                    &resolved.id,
                    protocol,
                    ConnectionStatus::Connected,
                    None,
                );
                if let Some(scope) = open_scope.as_ref() {
                    state.finish_connection_open(open_request_id, scope);
                }
                // 会话选项可裁剪能力（如关闭运行时指标），响应必须返回该会话的
                // 实际能力；用协议默认值会让前端发起后端已禁用的探测。
                let capabilities = state
                    .sessions()
                    .get(&session_id)
                    .map(|session| session.capabilities.clone())
                    .unwrap_or_else(|| driver.capabilities());
                Ok(ConnectionOpenResponse::Connected {
                    connection_id: resolved.id,
                    session_id,
                    protocol,
                    capabilities,
                    serial_port,
                    baud_rate,
                    serial_scores,
                })
            }
            Ok(ConnectionOpenResult::HostKeyPrompt {
                host,
                port,
                algorithm,
                fingerprint,
            }) => {
                if !open_scope
                    .as_ref()
                    .is_some_and(|scope| state.connection_open_matches(open_request_id, scope))
                {
                    protocol_registry().discard_pending_connections(&resolved.id);
                    if let Some(scope) = open_scope.as_ref() {
                        state.finish_connection_open(open_request_id, scope);
                    }
                    log.clone()
                        .field("host", &host)
                        .field("port", port)
                        .field("result", "stale_host_key_prompt_discarded")
                        .warn();
                    return Err(TerminalApiError::invalid("connection open was superseded"));
                }
                log.clone()
                    .field("host", &host)
                    .field("port", port)
                    .field("algorithm", &algorithm)
                    .warn();
                emit_connection_host_key_challenge(
                    &app,
                    ConnectionHostKeyChallengePayload {
                        connection_id: resolved.id.clone(),
                        session_id: open_request_id.to_string(),
                        host,
                        port,
                        algorithm,
                        fingerprint,
                    },
                )?;
                Ok(ConnectionOpenResponse::HostKeyChallenge {
                    awaiting: "hostKeyChallenge",
                    connection_id: resolved.id,
                    protocol,
                })
            }
            Err(error) => {
                if error.is_cancelled() {
                    if let Some(scope) = open_scope.as_ref() {
                        state.finish_connection_open(open_request_id, scope);
                    }
                    log.clone()
                        .field("error_code", error.code)
                        .field("result", "cancelled_open_ignored")
                        .debug();
                    return Err(TerminalApiError::invalid("connection open was cancelled"));
                }
                if !open_scope
                    .as_ref()
                    .is_some_and(|scope| state.connection_open_matches(open_request_id, scope))
                {
                    if let Some(scope) = open_scope.as_ref() {
                        state.finish_connection_open(open_request_id, scope);
                    }
                    log.clone()
                        .field("error_code", error.code)
                        .field("result", "stale_error_ignored")
                        .warn();
                    return Err(TerminalApiError::invalid("connection open was superseded"));
                }
                log.clone()
                    .field("recoverable", error.retryable)
                    .field("error_code", error.code)
                    .field("detail", error.detail.clone())
                    .error();
                set_connection_state(
                    state,
                    &resolved.id,
                    protocol,
                    ConnectionStatus::Failed,
                    Some(error.detail.clone()),
                );
                if let Some(scope) = open_scope.as_ref() {
                    state.finish_connection_open(open_request_id, scope);
                }
                Err(error.into())
            }
        }
    }

    pub(crate) async fn authenticate(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ConnectionAuthenticateCommand,
    ) -> Result<ConnectionOpenResponse, TerminalApiError> {
        let log = logging::event("terminal.connection_service", "connection.authenticate")
            .field("connection_id", &request.connection_id)
            .field("trust_host_key", request.trust_host_key.unwrap_or(false))
            .field(
                "accept_host_key_once",
                request.accept_host_key_once.unwrap_or(false),
            );
        log.clone().info();

        let open_scope = state
            .current_connection_open_scope(
                request
                    .open_request_id
                    .as_deref()
                    .unwrap_or(&request.connection_id),
            )
            .ok_or_else(|| TerminalApiError::invalid("connection open was superseded"))?;
        let open_request = ConnectionOpenRequest {
            connection_id: request.connection_id,
            open_request_id: request.open_request_id,
            trust_host_key: request.trust_host_key,
            accept_host_key_once: request.accept_host_key_once,
            terminal_scrollback: request.terminal_scrollback,
            terminal_type: None,
            encoding: None,
            realtime_encoding_detection: None,
            cols: request.cols,
            rows: request.rows,
            ssh_credential: request.ssh_credential,
        };
        let resolved =
            resolve_connection_request(state, open_request).map_err(TerminalApiError::from)?;
        self.connect(app, state, resolved.with_open_scope(open_scope), log)
            .await
    }

    pub(crate) fn close(
        &self,
        _app: AppHandle,
        state: &AppState,
        connection_id: &str,
    ) -> Result<(), TerminalApiError> {
        let log = logging::event("terminal.connection_service", "connection.close")
            .field("connection_id", connection_id);
        let protocol = state
            .connection_runtime(connection_id)
            .and_then(|runtime| ProtocolKind::from_str(&runtime.protocol));
        if let Some(protocol) = protocol {
            log.clone().field("protocol", protocol.as_str()).info();
        } else {
            log.clone().info();
        }

        state.forget_transient_connection(connection_id);
        state.cancel_connection_open(connection_id);
        protocol_registry().discard_pending_connections(connection_id);
        if let Some(protocol) = protocol {
            set_connection_state(
                state,
                connection_id,
                protocol,
                ConnectionStatus::Disconnecting,
                None,
            );
        }
        let session_ids = state.session_ids_for_connection(connection_id);
        for sid in &session_ids {
            log.clone().field("session_id", sid).debug();
            let _ = session_service().close(state, sid)?;
            state.remove_terminal_output_channels(sid);
        }
        if session_ids.is_empty() {
            if let Some(protocol) = protocol {
                set_connection_state(
                    state,
                    connection_id,
                    protocol,
                    ConnectionStatus::Disconnected,
                    None,
                );
            }
        }
        Ok(())
    }

    pub(crate) async fn start_metrics(
        &self,
        app: AppHandle,
        state: &AppState,
        request: SshRuntimeMetricsRequest,
    ) -> Result<(), TerminalApiError> {
        let connection_id = request.connection_id.clone();
        logging::event("terminal.connection_service", "metrics.start")
            .field("connection_id", &connection_id)
            .field("session_id", &request.session_id)
            .info();
        let protocol = state
            .connection_runtime(&connection_id)
            .and_then(|r| ProtocolKind::from_str(&r.protocol))
            .ok_or_else(|| {
                TerminalApiError::from(ConnectionError::connection_not_active(
                    "connection is not active",
                ))
            })?;
        protocol_registry()
            .require_driver(protocol)
            .start_metrics(app, state, request)
            .await
            .map_err(TerminalApiError::from)
    }

    pub(crate) fn stop_metrics(
        &self,
        state: &AppState,
        request: SshRuntimeMetricsRequest,
    ) -> Result<(), TerminalApiError> {
        let connection_id = request.connection_id.clone();
        logging::event("terminal.connection_service", "metrics.stop")
            .field("connection_id", &connection_id)
            .field("session_id", &request.session_id)
            .info();
        let Some(protocol) = state
            .connection_runtime(&connection_id)
            .and_then(|r| ProtocolKind::from_str(&r.protocol))
        else {
            state.remove_monitor_task(&request.session_id);
            return Ok(());
        };
        protocol_registry()
            .require_driver(protocol)
            .stop_metrics(state, request)
            .map_err(TerminalApiError::from)
    }
}

fn set_connection_state(
    state: &AppState,
    connection_id: &str,
    protocol: ProtocolKind,
    status: ConnectionStatus,
    last_error: Option<String>,
) {
    state.set_connection_runtime(
        connection_id,
        protocol.as_str().to_string(),
        status,
        last_error,
    );
}
