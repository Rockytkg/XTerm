use std::collections::VecDeque;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{
    state::{AppState, ConnectionStatus},
    terminal::{domain::ProtocolKind, events::SessionLifecycle},
};

use super::{
    codec::{decode_backend_bytes_with_raw, encode_for_backend},
    core::{
        CodecState, SessionCapabilityCommand, SessionCommand, SessionTransport, SessionWorkerEvent,
        TerminalResize, TerminalSession, TerminalSessionResources, TransportCapabilityCommand,
        TransportCommand,
    },
    delivery::{
        drain_live_output, drain_terminal_replay, emit_terminal_data, flush_terminal_output,
        should_flush_live_output, SessionDeliveryState,
    },
    startup_auth::{StartupAuthState, StartupPasswordAuth},
};

/// 自动登录窗口内暂存的前端输入条数上限（与前端 pendingInputs 一致）：超限丢弃
/// 最旧的输入，保留较新的按键更贴近用户当前意图。
const STARTUP_AUTH_HELD_INPUT_MAX: usize = 256;
pub(crate) struct SessionWorkerOptions {
    pub(crate) session_id: String,
    pub(crate) transport: SessionTransport,
    pub(crate) capabilities: crate::terminal::domain::ConnectionCapabilities,
    pub(crate) codec: CodecState,
    pub(crate) initial_data: Option<String>,
    pub(crate) startup_auth: Option<StartupPasswordAuth>,
    pub(crate) resources: TerminalSessionResources,
}

pub(crate) struct BoundSessionOptions {
    pub(crate) session_prefix: &'static str,
    pub(crate) connection_id: String,
    pub(crate) transport: SessionTransport,
    pub(crate) capabilities: crate::terminal::domain::ConnectionCapabilities,
    pub(crate) codec: CodecState,
    pub(crate) initial_data: Option<String>,
    pub(crate) startup_auth: Option<StartupPasswordAuth>,
    pub(crate) resources: TerminalSessionResources,
}

pub(crate) fn spawn_bound_session(
    app: AppHandle,
    state: &AppState,
    options: BoundSessionOptions,
) -> String {
    // A saved connection profile is a template and may back multiple terminal
    // tabs. Reconnect cleanup is scoped to the frontend session/attempt, so a
    // newly opened backend session must not supersede its siblings here.
    let session_id = crate::ids::new_id();
    state.bind_session_connection(&session_id, &options.connection_id);
    log::trace!(
        target: "terminal.runtime",
        "terminal.session.bound connection_id={} session_id={} protocol={}",
        options.connection_id,
        session_id,
        options.session_prefix
    );
    let session = spawn_session_worker(
        app,
        SessionWorkerOptions {
            session_id: session_id.clone(),
            transport: options.transport,
            capabilities: options.capabilities,
            codec: options.codec,
            initial_data: options.initial_data,
            startup_auth: options.startup_auth,
            resources: options.resources,
        },
    );
    state.sessions().insert(session_id.clone(), session);
    session_id
}

pub(crate) fn spawn_session_worker(
    app: AppHandle,
    options: SessionWorkerOptions,
) -> TerminalSession {
    let SessionWorkerOptions {
        session_id,
        transport,
        capabilities,
        mut codec,
        initial_data,
        startup_auth,
        resources,
    } = options;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    log::trace!(target: "terminal.runtime", "spawning backend worker for session '{session_id}'");
    let worker = tokio::spawn(async move {
        run_session_worker(SessionWorkerRuntime {
            app,
            session_id,
            transport,
            rx,
            codec: &mut codec,
            initial_data,
            startup_auth,
        })
        .await
    });
    TerminalSession {
        tx,
        capabilities,
        resources,
        worker: std::sync::Arc::new(worker),
    }
}

struct SessionWorkerRuntime<'a> {
    app: AppHandle,
    session_id: String,
    transport: SessionTransport,
    rx: tokio::sync::mpsc::UnboundedReceiver<SessionCommand>,
    codec: &'a mut CodecState,
    initial_data: Option<String>,
    startup_auth: Option<StartupPasswordAuth>,
}

struct SessionWorkerLoop<'a> {
    app: AppHandle,
    session_id: String,
    transport_tx: tokio::sync::mpsc::UnboundedSender<TransportCommand>,
    codec: &'a mut CodecState,
    delivery: SessionDeliveryState,
    startup_auth: Option<StartupAuthState>,
    /// 自动登录窗口内被暂存的前端输入，窗口关闭后按到达顺序补发。
    startup_auth_held_input: VecDeque<Vec<u8>>,
    last_transport_resize: Option<TerminalResize>,
    supports_pixel_resize: bool,
}

async fn run_session_worker(runtime: SessionWorkerRuntime<'_>) {
    let SessionWorkerRuntime {
        app,
        session_id,
        transport,
        mut rx,
        codec,
        initial_data,
        startup_auth,
    } = runtime;
    let supports_pixel_resize = transport.supports_pixel_resize();
    let raw_bytes_supported = transport.supports_raw_bytes();
    let last_transport_resize = transport.initial_size().map(|size| TerminalResize {
        cols: size.cols,
        rows: size.rows,
        width_px: None,
        height_px: None,
    });
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (transport_tx, transport_rx) = tokio::sync::mpsc::unbounded_channel();
    transport.spawn(session_id.clone(), transport_rx, event_tx.clone());

    let connection_id = app
        .state::<AppState>()
        .connection_id_for_session(&session_id)
        .unwrap_or_default();
    let delivery = SessionDeliveryState::new(connection_id, raw_bytes_supported);
    let startup_auth = startup_auth.map(StartupAuthState::new);
    let mut session_loop = SessionWorkerLoop {
        app,
        session_id,
        transport_tx,
        codec,
        delivery,
        startup_auth,
        startup_auth_held_input: VecDeque::new(),
        last_transport_resize,
        supports_pixel_resize,
    };
    log::debug!(
        target: "terminal.runtime",
        "backend session '{}' worker started",
        session_loop.session_id
    );
    if let Some(data) = initial_data {
        let startup_auth_payload =
            prepare_startup_auth_write(&mut session_loop.startup_auth, &data);
        emit_terminal_data(
            &session_loop.app,
            &session_loop.session_id,
            &mut session_loop.delivery,
            data,
            "utf-8".to_string(),
            None,
        );
        send_startup_auth_payload(&session_loop.transport_tx, startup_auth_payload);
    }

    loop {
        let replay_pending = session_loop.delivery.output_ready_channel_id.is_some()
            && session_loop.delivery.replay_channel_id.is_some()
            && session_loop.delivery.delivered_offset < session_loop.delivery.next_offset;
        let input_hold_until = session_loop
            .startup_auth
            .as_ref()
            .and_then(StartupAuthState::input_hold_until);
        let event =
            next_session_event(&mut rx, &mut event_rx, replay_pending, input_hold_until).await;

        match event {
            SessionLoopEvent::Command(command) => {
                if !handle_session_command(&mut session_loop, command).await {
                    return;
                }
            }
            SessionLoopEvent::Worker(event) => {
                match handle_worker_event(&mut session_loop, event) {
                    WorkerEventOutcome::Continue => {}
                    WorkerEventOutcome::SkipDrain => continue,
                    WorkerEventOutcome::Stop => return,
                }
            }
            SessionLoopEvent::StartupAuthHoldExpired => {
                // 自动登录窗口到期（对端始终未出现可识别的登录提示）：放弃认证，
                // 按到达顺序补发暂存输入，会话完全交还给用户。
                session_loop.startup_auth = None;
                if !flush_startup_auth_held_input(&mut session_loop) {
                    return;
                }
            }
            SessionLoopEvent::CommandChannelClosed => {
                finish_session_worker(
                    &mut session_loop,
                    SessionLifecycle::Failed,
                    Some("frontend command channel disconnected".to_string()),
                    true,
                );
                return;
            }
            SessionLoopEvent::WorkerChannelClosed => {
                finish_session_worker(
                    &mut session_loop,
                    SessionLifecycle::Failed,
                    Some("backend worker event channel disconnected".to_string()),
                    false,
                );
                return;
            }
        }

        drain_terminal_replay(
            &session_loop.app,
            &session_loop.session_id,
            &mut session_loop.delivery,
        );
        if session_loop.delivery.replay_channel_id.is_none()
            && should_flush_live_output(&session_loop.delivery)
        {
            drain_live_output(
                &session_loop.app,
                &session_loop.session_id,
                &mut session_loop.delivery,
            );
        }
    }
}

async fn handle_session_command(
    session_loop: &mut SessionWorkerLoop<'_>,
    command: SessionCommand,
) -> bool {
    match command {
        SessionCommand::Activate { channel_id, reply } => {
            session_loop.delivery.active_channel_id = Some(channel_id);
            session_loop.delivery.last_input_sequence = None;
            session_loop.delivery.output_ready_channel_id = None;
            session_loop.delivery.replay_channel_id = (session_loop.delivery.delivered_offset
                < session_loop.delivery.next_offset)
                .then_some(channel_id);
            // Re-arm raw output on the fresh channel: the frontend does not
            // resend SetRawOutput after a re-attach.
            session_loop.delivery.raw_output_channel_id = session_loop
                .delivery
                .raw_output_enabled
                .then_some(channel_id);
            let _ = reply.send(Ok(()));
            true
        }
        SessionCommand::Deactivate { channel_id, reply } => {
            if channel_id.is_none() || session_loop.delivery.active_channel_id == channel_id {
                drain_live_output(
                    &session_loop.app,
                    &session_loop.session_id,
                    &mut session_loop.delivery,
                );
                session_loop.delivery.active_channel_id = None;
                session_loop.delivery.output_ready_channel_id = None;
                session_loop.delivery.replay_channel_id = None;
            }
            if channel_id.is_none() || session_loop.delivery.raw_output_channel_id == channel_id {
                session_loop.delivery.raw_output_channel_id = None;
            }
            let _ = reply.send(Ok(()));
            true
        }
        SessionCommand::Write {
            channel_id,
            input_sequence,
            data,
        } => {
            if !accept_input_sequence(&mut session_loop.delivery, channel_id, input_sequence) {
                return true;
            }
            let bytes = encode_for_backend(&data, session_loop.codec);
            forward_or_hold_session_input(session_loop, bytes)
        }
        SessionCommand::WriteBytes {
            channel_id,
            input_sequence,
            data,
        } => {
            if !accept_input_sequence(&mut session_loop.delivery, channel_id, input_sequence) {
                return true;
            }
            forward_or_hold_session_input(session_loop, data)
        }
        SessionCommand::FlushOutput => {
            session_loop.delivery.output_ready_channel_id = session_loop.delivery.active_channel_id;
            drain_terminal_replay(
                &session_loop.app,
                &session_loop.session_id,
                &mut session_loop.delivery,
            );
            drain_live_output(
                &session_loop.app,
                &session_loop.session_id,
                &mut session_loop.delivery,
            );
            true
        }
        SessionCommand::SetEncodingDetection {
            channel_id,
            enabled,
            encoding,
        } => {
            if channel_id.is_none() || session_loop.delivery.active_channel_id == channel_id {
                if let Some(encoding) = encoding {
                    session_loop.codec.set_backend_encoding(encoding);
                } else {
                    session_loop.codec.clear_backend_encoding();
                }
                session_loop.codec.set_realtime_detection(enabled);
            }
            true
        }
        SessionCommand::SetRawOutput {
            channel_id,
            enabled,
        } => handle_raw_output_command(session_loop, channel_id, enabled),
        SessionCommand::InvokeCapability(command) => {
            handle_session_capability_command(session_loop, command)
        }
        SessionCommand::Resize {
            channel_id,
            cols,
            rows,
            width_px,
            height_px,
        } => {
            if session_loop.delivery.active_channel_id != channel_id {
                return true;
            }
            let resize = TerminalResize {
                cols,
                rows,
                width_px,
                height_px,
            };
            if !should_forward_resize(
                session_loop.last_transport_resize.as_mut(),
                resize,
                session_loop.supports_pixel_resize,
            ) {
                return true;
            }
            forward_transport_command(
                &session_loop.app,
                &session_loop.session_id,
                &session_loop.transport_tx,
                TransportCommand::Resize(resize),
            )
        }
        SessionCommand::Close => {
            finish_session_worker(session_loop, SessionLifecycle::Closed, None, true);
            false
        }
    }
}

fn accept_input_sequence(
    delivery: &mut SessionDeliveryState,
    channel_id: Option<u64>,
    input_sequence: Option<u64>,
) -> bool {
    if delivery.active_channel_id != channel_id {
        return false;
    }
    let Some(input_sequence) = input_sequence else {
        return true;
    };
    if delivery
        .last_input_sequence
        .is_some_and(|last| input_sequence <= last)
    {
        return false;
    }
    delivery.last_input_sequence = Some(input_sequence);
    true
}

fn handle_raw_output_command(
    session_loop: &mut SessionWorkerLoop<'_>,
    channel_id: Option<u64>,
    enabled: bool,
) -> bool {
    if !session_loop.delivery.raw_bytes_supported {
        return true;
    }
    if channel_id.is_some() && session_loop.delivery.active_channel_id != channel_id {
        return true;
    }
    drain_live_output(
        &session_loop.app,
        &session_loop.session_id,
        &mut session_loop.delivery,
    );
    session_loop.delivery.raw_output_enabled = enabled;
    session_loop.delivery.raw_output_channel_id =
        enabled.then_some(channel_id).flatten().or_else(|| {
            enabled
                .then_some(session_loop.delivery.output_ready_channel_id)
                .flatten()
        });
    true
}

fn handle_session_capability_command(
    session_loop: &mut SessionWorkerLoop<'_>,
    command: SessionCapabilityCommand,
) -> bool {
    match command {
        SessionCapabilityCommand::RedetectSerialBaud { reply } => {
            flush_terminal_output(
                &session_loop.app,
                &session_loop.session_id,
                session_loop.codec,
                &mut session_loop.delivery,
            );
            forward_transport_command(
                &session_loop.app,
                &session_loop.session_id,
                &session_loop.transport_tx,
                TransportCommand::InvokeCapability(
                    TransportCapabilityCommand::RedetectSerialBaud {
                        encoding: session_loop.codec.backend_encoding.clone(),
                        reply,
                    },
                ),
            )
        }
    }
}

enum WorkerEventOutcome {
    Continue,
    SkipDrain,
    Stop,
}

fn handle_worker_event(
    session_loop: &mut SessionWorkerLoop<'_>,
    event: SessionWorkerEvent,
) -> WorkerEventOutcome {
    match event {
        SessionWorkerEvent::Ready => {
            emit_session_ready(&session_loop.app, &session_loop.session_id);
            WorkerEventOutcome::Continue
        }
        SessionWorkerEvent::Data {
            bytes,
            negotiated_encoding,
        } => {
            if let Some(encoding) = negotiated_encoding {
                session_loop.codec.set_backend_encoding(encoding);
            }
            if bytes.is_empty() {
                return WorkerEventOutcome::SkipDrain;
            }
            let payload = decode_backend_bytes_with_raw(
                &bytes,
                session_loop.codec,
                session_loop.delivery.raw_bytes_mode(),
            );
            let startup_auth_payload =
                prepare_startup_auth_write(&mut session_loop.startup_auth, &payload.data);
            emit_terminal_data(
                &session_loop.app,
                &session_loop.session_id,
                &mut session_loop.delivery,
                payload.data,
                payload.encoding,
                Some(payload.raw_bytes),
            );
            send_startup_auth_payload(&session_loop.transport_tx, startup_auth_payload);
            // 凭据行落线（若有）后检查持有窗口：认证完成、或流程转入只能由用户
            // 推进的阶段（密码-only 配置见到了用户名提示）都会关闭窗口，补发
            // 暂存输入。
            let hold_open = session_loop
                .startup_auth
                .as_ref()
                .and_then(StartupAuthState::input_hold_until)
                .is_some();
            if !hold_open
                && !session_loop.startup_auth_held_input.is_empty()
                && !flush_startup_auth_held_input(session_loop)
            {
                return WorkerEventOutcome::Stop;
            }
            WorkerEventOutcome::Continue
        }
        SessionWorkerEvent::Closed(reason) => {
            finish_session_worker(session_loop, SessionLifecycle::Closed, reason, true);
            WorkerEventOutcome::Stop
        }
        SessionWorkerEvent::Failed(error) => {
            finish_session_worker(session_loop, SessionLifecycle::Failed, Some(error), true);
            WorkerEventOutcome::Stop
        }
    }
}

fn emit_session_ready(app: &AppHandle, session_id: &str) {
    let state = app.state::<AppState>();
    let Some(connection_id) = state.connection_id_for_session(session_id) else {
        return;
    };
    if let Err(error) = crate::terminal::events::emit_session_lifecycle(
        app,
        state.inner(),
        &connection_id,
        session_id,
        SessionLifecycle::Ready,
        None,
    ) {
        log::warn!(target: "terminal.runtime", "failed to publish ready state for backend session '{session_id}': {error}");
    }
}

fn finish_session_worker(
    session_loop: &mut SessionWorkerLoop<'_>,
    lifecycle: SessionLifecycle,
    reason: Option<String>,
    close_transport: bool,
) {
    if close_transport {
        close_transport_silently(&session_loop.transport_tx);
    }
    flush_terminal_output(
        &session_loop.app,
        &session_loop.session_id,
        session_loop.codec,
        &mut session_loop.delivery,
    );
    emit_session_status(
        &session_loop.app,
        &session_loop.session_id,
        lifecycle,
        reason,
    );
}

fn prepare_startup_auth_write(
    startup_auth: &mut Option<StartupAuthState>,
    text: &str,
) -> Option<Vec<u8>> {
    let state = startup_auth.as_mut()?;
    let payload = state.observe(text);
    if state.is_finished() {
        *startup_auth = None;
    }
    payload
}

/// 自动登录窗口内的前端输入一律暂存而不是直接落线：窗口内到达的字节（用户
/// 提前敲的按键、xterm 对设备查询序列的自动应答等）若插在自动填写的用户名/
/// 密码前面，会把凭据行顶成 "xxadmin" 之类导致登录失败。窗口结束后按到达
/// 顺序补发，既不丢输入也不污染凭据行。
fn forward_or_hold_session_input(session_loop: &mut SessionWorkerLoop<'_>, bytes: Vec<u8>) -> bool {
    let holding = session_loop
        .startup_auth
        .as_ref()
        .and_then(StartupAuthState::input_hold_until)
        .is_some();
    if !holding && session_loop.startup_auth_held_input.is_empty() {
        return forward_transport_write(
            &session_loop.app,
            &session_loop.session_id,
            &session_loop.transport_tx,
            bytes,
        );
    }
    if session_loop.startup_auth_held_input.len() >= STARTUP_AUTH_HELD_INPUT_MAX {
        session_loop.startup_auth_held_input.pop_front();
    }
    session_loop.startup_auth_held_input.push_back(bytes);
    if holding {
        return true;
    }
    // 窗口刚好到期：新输入连同暂存一起按序补发，保持到达顺序。
    flush_startup_auth_held_input(session_loop)
}

fn flush_startup_auth_held_input(session_loop: &mut SessionWorkerLoop<'_>) -> bool {
    while let Some(bytes) = session_loop.startup_auth_held_input.pop_front() {
        if !forward_transport_write(
            &session_loop.app,
            &session_loop.session_id,
            &session_loop.transport_tx,
            bytes,
        ) {
            return false;
        }
    }
    true
}

fn send_startup_auth_payload(
    transport_tx: &tokio::sync::mpsc::UnboundedSender<TransportCommand>,
    payload: Option<Vec<u8>>,
) {
    if let Some(payload) = payload {
        let _ = send_transport_write(transport_tx, payload);
    }
}

fn send_transport_write(
    transport_tx: &tokio::sync::mpsc::UnboundedSender<TransportCommand>,
    bytes: Vec<u8>,
) -> bool {
    transport_tx.send(TransportCommand::Write(bytes)).is_ok()
}

fn forward_transport_write(
    app: &AppHandle,
    session_id: &str,
    transport_tx: &tokio::sync::mpsc::UnboundedSender<TransportCommand>,
    bytes: Vec<u8>,
) -> bool {
    if send_transport_write(transport_tx, bytes) {
        return true;
    }
    emit_transport_stopped(app, session_id);
    false
}

fn forward_transport_command(
    app: &AppHandle,
    session_id: &str,
    transport_tx: &tokio::sync::mpsc::UnboundedSender<TransportCommand>,
    command: TransportCommand,
) -> bool {
    if transport_tx.send(command).is_ok() {
        return true;
    }
    emit_transport_stopped(app, session_id);
    false
}

fn emit_transport_stopped(app: &AppHandle, session_id: &str) {
    emit_session_status(
        app,
        session_id,
        SessionLifecycle::Failed,
        Some("backend transport writer stopped".to_string()),
    );
}

fn close_transport_silently(transport_tx: &tokio::sync::mpsc::UnboundedSender<TransportCommand>) {
    let _ = transport_tx.send(TransportCommand::Close);
}

fn should_forward_resize(
    last_resize: Option<&mut TerminalResize>,
    resize: TerminalResize,
    compare_pixels: bool,
) -> bool {
    let Some(last_resize) = last_resize else {
        return false;
    };
    let same_size = last_resize.cols == resize.cols && last_resize.rows == resize.rows;
    let same_pixels = !compare_pixels
        || (last_resize.width_px == resize.width_px && last_resize.height_px == resize.height_px);
    if same_size && same_pixels {
        return false;
    }
    *last_resize = resize;
    true
}

enum SessionLoopEvent {
    Command(SessionCommand),
    CommandChannelClosed,
    Worker(SessionWorkerEvent),
    WorkerChannelClosed,
    StartupAuthHoldExpired,
}

async fn next_session_event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionCommand>,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionWorkerEvent>,
    replay_pending: bool,
    input_hold_until: Option<std::time::Instant>,
) -> SessionLoopEvent {
    // 无持有窗口时该分支永不就绪；窗口存在时保证截止时刻必被观察到，
    // 即使对端此后不再输出、用户也不再输入。
    let hold_expired = async move {
        match input_hold_until {
            Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
            None => std::future::pending().await,
        }
    };
    if replay_pending {
        tokio::select! {
            biased;
            command = rx.recv() => {
                command.map(SessionLoopEvent::Command).unwrap_or(SessionLoopEvent::CommandChannelClosed)
            }
            event = event_rx.recv() => {
                event.map(SessionLoopEvent::Worker).unwrap_or(SessionLoopEvent::WorkerChannelClosed)
            }
            _ = hold_expired => SessionLoopEvent::StartupAuthHoldExpired,
        }
    } else {
        tokio::select! {
            event = event_rx.recv() => {
                event.map(SessionLoopEvent::Worker).unwrap_or(SessionLoopEvent::WorkerChannelClosed)
            }
            command = rx.recv() => {
                command.map(SessionLoopEvent::Command).unwrap_or(SessionLoopEvent::CommandChannelClosed)
            }
            _ = hold_expired => SessionLoopEvent::StartupAuthHoldExpired,
        }
    }
}

pub(super) fn emit_session_status(
    app: &AppHandle,
    session_id: &str,
    lifecycle: SessionLifecycle,
    reason: Option<String>,
) {
    let state = app.state::<AppState>();
    let connection_id = state
        .connection_id_for_session(session_id)
        .unwrap_or_default();
    if !connection_id.is_empty() {
        if let Err(error) = crate::terminal::events::emit_session_lifecycle(
            app,
            state.inner(),
            &connection_id,
            session_id,
            lifecycle,
            reason.clone(),
        ) {
            log::warn!(
                target: "terminal.runtime",
                "failed to publish '{}' for backend session '{session_id}': {error}",
                lifecycle.as_str()
            );
        }
    }
    if let Err(error) = cleanup_session_state(app, session_id) {
        log::warn!(target: "terminal.runtime", "failed to clean backend session state for '{session_id}': {error}");
    }
    if connection_id.is_empty() {
        app.state::<AppState>()
            .remove_terminal_output_channels(session_id);
        return;
    }
    update_connection_status_after_session_end(app, &connection_id, lifecycle, reason.clone());
    app.state::<AppState>()
        .remove_terminal_output_channels(session_id);
}

fn update_connection_status_after_session_end(
    app: &AppHandle,
    connection_id: &str,
    lifecycle: SessionLifecycle,
    reason: Option<String>,
) {
    if connection_id.is_empty() {
        return;
    }

    let state = app.state::<AppState>();
    let remaining_sessions = state.session_ids_for_connection(connection_id);
    if !remaining_sessions.is_empty() {
        return;
    }
    // A superseding open may be in flight (force-reconnect): the old session
    // ending must not clobber the new attempt's connecting/connected state.
    if state.current_connection_open_scope(connection_id).is_some() {
        return;
    }
    // Deep-link (transient) connections have no persisted profile; once their
    // last session is gone, drop the resolved copy (it may hold inline
    // passwords) instead of keeping it for the app's whole lifetime. This is
    // a no-op for workspace profile connections.
    state.forget_transient_connection(connection_id);

    let Some(protocol) = state
        .connection_runtime(connection_id)
        .and_then(|runtime| ProtocolKind::from_str(&runtime.protocol))
    else {
        return;
    };
    let next_state = if lifecycle == SessionLifecycle::Failed {
        ConnectionStatus::Failed
    } else {
        ConnectionStatus::Disconnected
    };
    state.set_connection_runtime(
        connection_id,
        protocol.as_str().to_string(),
        next_state,
        reason,
    );
}

pub(super) fn cleanup_session_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    session_id: &str,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let session = state.sessions().remove(session_id);
    if let Some(session) = session.as_ref() {
        session.resources.dispose();
    }
    state.release_session_resources(session_id, session.as_ref());
    Ok(())
}

/// 通知所有会话关闭并返回排空任务的句柄：调用方必须在进程退出前 await，
/// 否则 2s 优雅排空（含卡死 worker 的 abort 与 cleanup_session_state）
/// 会被退出截断。
pub(crate) fn shutdown_all_sessions<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::async_runtime::JoinHandle<()> {
    let sessions: Vec<(String, TerminalSession)> = app
        .state::<AppState>()
        .sessions()
        .iter()
        .map(|(session_id, session)| (session_id.clone(), session.clone()))
        .collect();

    for (session_id, session) in &sessions {
        if let Err(error) = session.tx.send(SessionCommand::Close) {
            log::debug!(target: "terminal.runtime", "failed to signal backend session '{session_id}' to close: {error}");
        }
    }

    // Wait for workers (and, via the closed command channel, their transport
    // actors) to exit before the final cleanup, with a bounded fallback so
    // shutdown cannot hang on a stuck session.
    const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let deadline = std::time::Instant::now() + SHUTDOWN_DRAIN_TIMEOUT;
        loop {
            if sessions
                .iter()
                .all(|(_, session)| session.worker.is_finished())
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                let pending: Vec<&str> = sessions
                    .iter()
                    .filter(|(_, session)| !session.worker.is_finished())
                    .map(|(session_id, _)| session_id.as_str())
                    .collect();
                log::warn!(target: "terminal.runtime", "timed out waiting for backend sessions to stop: {pending:?}");
                // The worker owns the transport command sender. If it is
                // stuck, merely removing the session leaves the transport
                // actor (and resources such as a serial fd) running forever.
                // Aborting the worker drops that sender, so each actor sees
                // a closed command channel and releases its own resources.
                for (session_id, session) in &sessions {
                    if !session.worker.is_finished() {
                        log::warn!(
                            target: "terminal.runtime",
                            "aborting stuck backend session worker '{session_id}'"
                        );
                        session.worker.abort();
                    }
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        for (session_id, _) in &sessions {
            if let Err(error) = cleanup_session_state(&app, session_id) {
                log::warn!(
                    target: "terminal.runtime",
                    "failed to clean backend session '{session_id}' during shutdown: {error}"
                );
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{accept_input_sequence, should_forward_resize, SessionDeliveryState};
    use crate::terminal::internal::core::TerminalResize;

    #[test]
    fn input_sequence_is_monotonic_within_a_channel() {
        let mut delivery = SessionDeliveryState::new("connection".to_string(), false);
        delivery.active_channel_id = Some(7);

        assert!(accept_input_sequence(&mut delivery, Some(7), Some(1)));
        assert!(!accept_input_sequence(&mut delivery, Some(7), Some(1)));
        assert!(accept_input_sequence(&mut delivery, Some(7), Some(2)));
        assert!(!accept_input_sequence(&mut delivery, Some(8), Some(3)));
    }

    #[test]
    fn legacy_input_without_a_sequence_remains_compatible() {
        let mut delivery = SessionDeliveryState::new("connection".to_string(), false);
        delivery.active_channel_id = Some(7);

        assert!(accept_input_sequence(&mut delivery, Some(7), None));
        assert!(accept_input_sequence(&mut delivery, Some(7), None));
    }

    #[test]
    fn resize_forwarding_includes_pixel_dimensions() {
        let mut last = Some(TerminalResize {
            cols: 120,
            rows: 40,
            width_px: Some(1200),
            height_px: Some(800),
        });

        assert!(should_forward_resize(
            last.as_mut(),
            TerminalResize {
                cols: 120,
                rows: 40,
                width_px: Some(1280),
                height_px: Some(800),
            },
            true,
        ));
        assert!(!should_forward_resize(
            last.as_mut(),
            TerminalResize {
                cols: 120,
                rows: 40,
                width_px: Some(1280),
                height_px: Some(800),
            },
            true,
        ));

        assert!(!should_forward_resize(
            last.as_mut(),
            TerminalResize {
                cols: 120,
                rows: 40,
                width_px: Some(1440),
                height_px: Some(900),
            },
            false,
        ));
    }
}
