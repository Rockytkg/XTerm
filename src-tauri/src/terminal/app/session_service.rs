use crate::{
    logging,
    state::AppState,
    terminal::{
        domain::{ConnectionCapabilities, TerminalApiError},
        internal::{SessionCommand, TerminalSession},
    },
};

#[derive(Clone, Copy, Default)]
pub(crate) struct SessionApplicationService;

pub(crate) struct TerminalChannelLease {
    pub(crate) connection_id: String,
    pub(crate) channel_id: u64,
    pub(crate) already_active: bool,
}

pub(crate) struct SessionResizeRequest {
    pub channel_id: Option<u64>,
    pub cols: u32,
    pub rows: u32,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
}

struct SessionCommandBus<'a> {
    state: &'a AppState,
}

#[derive(Clone, Copy)]
enum ChannelRequirement {
    Active,
    CurrentWhenPresent,
}

#[derive(Clone, Copy)]
enum SuccessLogLevel {
    Info,
    Trace,
}

struct ChannelCommand<'a> {
    session_id: &'a str,
    channel_id: Option<u64>,
    requirement: ChannelRequirement,
    event_name: &'static str,
    success_level: SuccessLogLevel,
    command: SessionCommand,
}

impl<'a> SessionCommandBus<'a> {
    fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    fn session(&self, session_id: &str) -> Result<TerminalSession, TerminalApiError> {
        self.state
            .sessions()
            .get(session_id)
            .cloned()
            .ok_or_else(|| TerminalApiError::invalid("session not found"))
    }

    fn capabilities(&self, session_id: &str) -> Result<ConnectionCapabilities, TerminalApiError> {
        Ok(self.session(session_id)?.capabilities)
    }

    fn active_channel(&self, session_id: &str) -> Option<u64> {
        self.state
            .session_runtime(session_id)
            .and_then(|runtime| runtime.active_channel_id)
    }

    fn channel_is_active(&self, session_id: &str, channel_id: Option<u64>) -> bool {
        self.active_channel(session_id) == channel_id
    }

    fn channel_is_current_when_present(&self, session_id: &str, channel_id: Option<u64>) -> bool {
        channel_id.is_none() || self.channel_is_active(session_id, channel_id)
    }

    fn channel_matches(
        &self,
        session_id: &str,
        channel_id: Option<u64>,
        requirement: ChannelRequirement,
    ) -> bool {
        match requirement {
            ChannelRequirement::Active => self.channel_is_active(session_id, channel_id),
            ChannelRequirement::CurrentWhenPresent => {
                self.channel_is_current_when_present(session_id, channel_id)
            }
        }
    }

    fn supports(
        &self,
        session_id: &str,
        capability: impl FnOnce(ConnectionCapabilities) -> bool,
    ) -> Result<bool, TerminalApiError> {
        Ok(capability(self.capabilities(session_id)?))
    }

    fn send(&self, session_id: &str, command: SessionCommand) -> Result<(), TerminalApiError> {
        self.session(session_id)?
            .tx
            .send(command)
            .map_err(|_| TerminalApiError::from("backend session is no longer running".to_string()))
    }

    fn send_channel_command(
        &self,
        request: ChannelCommand<'_>,
        log_details: impl FnOnce(crate::logging::LogEvent) -> crate::logging::LogEvent,
    ) -> Result<(), TerminalApiError> {
        let log = logging::event("terminal.session_service", request.event_name)
            .field("session_id", request.session_id)
            .maybe_field("channel_id", request.channel_id);
        if !self.channel_matches(request.session_id, request.channel_id, request.requirement) {
            log_details(log).field("cause", "inactive_channel").debug();
            return Ok(());
        }
        match request.success_level {
            SuccessLogLevel::Info => log_details(log).info(),
            SuccessLogLevel::Trace => log_details(log).trace(),
        }
        self.send(request.session_id, request.command)
    }
}

impl SessionApplicationService {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn activate_with_reserved_subscription(
        &self,
        state: &AppState,
        session_id: &str,
        subscription_id: Option<u64>,
    ) -> Result<TerminalChannelLease, TerminalApiError> {
        let request_log = logging::event("terminal.session_service", "session.activate")
            .field("session_id", session_id);
        let connection_id = state
            .connection_id_for_session(session_id)
            .ok_or_else(|| TerminalApiError::invalid("session not found"))?;
        let bus = SessionCommandBus::new(state);
        if let Some(channel_id) = bus.active_channel(session_id) {
            if let Some(subscription_id) = subscription_id {
                // The output subscription must own the channel before any flush/replay can run.
                if !state.bind_terminal_output_lease(session_id, subscription_id, channel_id) {
                    return Err(TerminalApiError::from(
                        "terminal output subscription was removed before activation completed"
                            .to_string(),
                    ));
                }
            }
            request_log
                .field("connection_id", &connection_id)
                .field("channel_id", channel_id)
                .field("result", "already_active")
                .debug();
            return Ok(TerminalChannelLease {
                connection_id,
                channel_id,
                already_active: true,
            });
        }
        let (channel_id, _already_active) = state.activate_session_channel(session_id);
        if let Some(subscription_id) = subscription_id {
            // Bind before notifying the worker; Activate may be followed by immediate replay.
            if !state.bind_terminal_output_lease(session_id, subscription_id, channel_id) {
                let _ = state.deactivate_session_channel(session_id, Some(channel_id));
                return Err(TerminalApiError::from(
                    "terminal output subscription was removed before activation completed"
                        .to_string(),
                ));
            }
        }
        request_log
            .clone()
            .field("connection_id", &connection_id)
            .field("channel_id", channel_id)
            .info();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        bus.send(
            session_id,
            SessionCommand::Activate {
                channel_id,
                reply: reply_tx,
            },
        )
        .inspect_err(|_| {
            let _ = state.deactivate_session_channel(session_id, Some(channel_id));
        })?;
        reply_rx
            .await
            .map_err(|_| {
                TerminalApiError::from("backend session activation reply dropped".to_string())
            })?
            .map_err(|error| {
                let _ = state.deactivate_session_channel(session_id, Some(channel_id));
                TerminalApiError::from(error)
            })?;
        request_log
            .field("connection_id", &connection_id)
            .field("channel_id", channel_id)
            .debug();
        Ok(TerminalChannelLease {
            connection_id,
            channel_id,
            already_active: false,
        })
    }

    pub(crate) async fn deactivate(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
    ) -> Result<Option<String>, TerminalApiError> {
        let request_log = logging::event("terminal.session_service", "session.deactivate")
            .field("session_id", session_id)
            .maybe_field("channel_id", channel_id);
        let connection_id = state.connection_id_for_session(session_id);
        let bus = SessionCommandBus::new(state);
        if bus.session(session_id).is_err() {
            let _ = state.deactivate_session_channel(session_id, channel_id);
            request_log
                .clone()
                .field("result", "session_missing_runtime_only")
                .warn();
            return Ok(connection_id);
        }
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        bus.send(
            session_id,
            SessionCommand::Deactivate {
                channel_id,
                reply: reply_tx,
            },
        )?;
        reply_rx
            .await
            .map_err(|_| {
                TerminalApiError::from("backend session deactivation reply dropped".to_string())
            })?
            .map_err(TerminalApiError::from)?;
        let _ = state.deactivate_session_channel(session_id, channel_id);
        request_log
            .field("connection_id", connection_id.clone().unwrap_or_default())
            .debug();
        Ok(connection_id)
    }

    pub(crate) fn write(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data: String,
    ) -> Result<(), TerminalApiError> {
        let data_len = data.len();
        let bus = SessionCommandBus::new(state);
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id,
                requirement: ChannelRequirement::CurrentWhenPresent,
                event_name: "session.write",
                success_level: SuccessLogLevel::Trace,
                command: SessionCommand::Write {
                    channel_id,
                    input_sequence,
                    data,
                },
            },
            |log| log.field("bytes", data_len),
        )
    }

    pub(crate) fn write_bytes(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data: Vec<u8>,
    ) -> Result<(), TerminalApiError> {
        let data_len = data.len();
        let bus = SessionCommandBus::new(state);
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id,
                requirement: ChannelRequirement::CurrentWhenPresent,
                event_name: "session.write_bytes",
                success_level: SuccessLogLevel::Trace,
                command: SessionCommand::WriteBytes {
                    channel_id,
                    input_sequence,
                    data,
                },
            },
            |log| log.field("bytes", data_len),
        )
    }

    pub(crate) fn flush_output(
        &self,
        state: &AppState,
        session_id: &str,
    ) -> Result<(), TerminalApiError> {
        SessionCommandBus::new(state).send(session_id, SessionCommand::FlushOutput)
    }

    pub(crate) fn resize(
        &self,
        state: &AppState,
        session_id: &str,
        request: SessionResizeRequest,
    ) -> Result<(), TerminalApiError> {
        let bus = SessionCommandBus::new(state);
        if !bus.supports(session_id, |capabilities| capabilities.resize)? {
            logging::event("terminal.session_service", "session.resize.ignored")
                .field("session_id", session_id)
                .maybe_field("channel_id", request.channel_id)
                .field("cols", request.cols)
                .field("rows", request.rows)
                .field("cause", "unsupported_protocol")
                .trace();
            return Ok(());
        }
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id: request.channel_id,
                requirement: ChannelRequirement::Active,
                event_name: "session.resize",
                success_level: SuccessLogLevel::Trace,
                command: SessionCommand::Resize {
                    channel_id: request.channel_id,
                    cols: request.cols.clamp(1, 1_000),
                    rows: request.rows.clamp(1, 1_000),
                    width_px: request.width_px,
                    height_px: request.height_px,
                },
            },
            |log| {
                log.field("cols", request.cols)
                    .field("rows", request.rows)
                    .maybe_field("width_px", request.width_px)
                    .maybe_field("height_px", request.height_px)
            },
        )
    }

    pub(crate) fn set_encoding_detection(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
        enabled: bool,
        encoding: Option<String>,
    ) -> Result<(), TerminalApiError> {
        let bus = SessionCommandBus::new(state);
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id,
                requirement: ChannelRequirement::Active,
                event_name: "session.encoding_detection",
                success_level: SuccessLogLevel::Info,
                command: SessionCommand::SetEncodingDetection {
                    channel_id,
                    enabled,
                    encoding,
                },
            },
            |log| log.field("enabled", enabled),
        )
    }

    pub(crate) fn set_raw_output(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
        enabled: bool,
    ) -> Result<(), TerminalApiError> {
        let bus = SessionCommandBus::new(state);
        if !bus.supports(session_id, |capabilities| capabilities.raw_output)? {
            logging::event("terminal.session_service", "session.raw_output.ignored")
                .field("session_id", session_id)
                .maybe_field("channel_id", channel_id)
                .field("enabled", enabled)
                .field("cause", "unsupported_protocol")
                .debug();
            return Ok(());
        }
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id,
                requirement: ChannelRequirement::CurrentWhenPresent,
                event_name: "session.raw_output",
                success_level: SuccessLogLevel::Trace,
                command: SessionCommand::SetRawOutput {
                    channel_id,
                    enabled,
                },
            },
            |log| log.field("enabled", enabled),
        )
    }

    pub(crate) fn rendered_offset(
        &self,
        state: &AppState,
        session_id: &str,
        channel_id: Option<u64>,
        offset: u64,
    ) -> Result<(), TerminalApiError> {
        let bus = SessionCommandBus::new(state);
        bus.send_channel_command(
            ChannelCommand {
                session_id,
                channel_id,
                requirement: ChannelRequirement::CurrentWhenPresent,
                event_name: "session.rendered_offset",
                success_level: SuccessLogLevel::Trace,
                command: SessionCommand::RenderedOffset {
                    channel_id: channel_id.unwrap_or(0),
                    offset,
                },
            },
            |log| log.field("offset", offset),
        )
    }

    pub(crate) async fn redetect_serial_baud(
        &self,
        state: &AppState,
        session_id: &str,
    ) -> Result<crate::terminal::internal::SerialRedetectResult, TerminalApiError> {
        let bus = SessionCommandBus::new(state);
        if !bus.supports(session_id, |capabilities| {
            capabilities.serial_baud_detection
        })? {
            return Err(TerminalApiError::unsupported(
                "serial baud redetect is not supported for this session",
            ));
        }
        logging::event("terminal.session_service", "session.redetect_serial_baud")
            .field("session_id", session_id)
            .info();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        bus.send(
            session_id,
            SessionCommand::InvokeCapability(
                crate::terminal::internal::SessionCapabilityCommand::RedetectSerialBaud {
                    reply: reply_tx,
                },
            ),
        )?;
        reply_rx
            .await
            .map_err(|_| {
                TerminalApiError::from("backend serial baud redetect reply dropped".to_string())
            })?
            .map_err(TerminalApiError::from)
    }

    pub(crate) fn close(
        &self,
        state: &AppState,
        session_id: &str,
    ) -> Result<Option<String>, TerminalApiError> {
        logging::event("terminal.session_service", "session.close")
            .field("session_id", session_id)
            .info();
        let connection_id = state.connection_id_for_session(session_id);
        if let Some(session) = state.sessions().get(session_id) {
            let _ = session.tx.send(SessionCommand::Close);
        }
        Ok(connection_id)
    }
}
