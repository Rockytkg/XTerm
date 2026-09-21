//! RDP 会话 actor：驱动 IronRDP 会话状态机，并通过回环 WebSocket 桥与前端通信。
//!
//! 与 VNC 的字节透传桥不同，RDP 的协议状态机完全在后端：IronRDP 解码图形更新到
//! 帧缓冲，脏矩形经桥帧协议（见 `codec.rs`）推给前端渲染；前端的键鼠/缩放/剪贴板
//! 消息在这里转成 RDP 输入 PDU 写回服务器。前端断开只影响推帧，RDP 会话继续消费，
//! 重连时重发 Hello + 全帧。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use ironrdp::cliprdr::backend::CliprdrBackend;
use ironrdp::cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
    FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
    OwnedFormatDataResponse,
};
use ironrdp::cliprdr::CliprdrClient;
use ironrdp::connector::connection_activation::{
    ConnectionActivationFactory, ConnectionActivationState,
};
use ironrdp::connector::{ConnectionResult, DesktopSize};
use ironrdp::core::WriteBuf;
use ironrdp::displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::input::{
    Database as InputDatabase, MouseButton, MousePosition, Operation, Scancode, WheelRotations,
};
use ironrdp::pdu::geometry::{InclusiveRectangle, Rectangle as _};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use ironrdp_tokio::{FramedWrite as _, TokioFramed};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use crate::terminal::internal::{
    core::{SessionTransportRuntime, SessionWorkerEvent, TerminalSize, TransportCommand},
    loopback_bridge::accept_bridge_client,
};

use super::codec::{ClientMessage, DirtyRect, ServerMessage};

/// TLS 升级后的 RDP 连接帧封装类型。
pub(super) type RdpFramed = TokioFramed<ironrdp_tls::TlsStream<TcpStream>>;

/// 桥客户端首帧（全帧可达数 MB）发送的上限；客户端停滞时超时丢弃，
/// 避免阻塞会话主循环（WS 握手本身在 `accept_bridge_client` 内另有超时）。
const BRIDGE_SETUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Deactivation-Reactivation 序列总时长上限；正常只有几个 PDU，
/// 对端不应答时不能无限等待。
const REACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);

/// 剪贴板后端发给会话 actor 的事件。backend 嵌在 CLIPRDR SVC 处理器内，
/// 没有网络写权限，所有需要写回服务器的动作都经此通道转交 actor 执行。
pub(super) enum ClipboardEvent {
    /// 远端剪贴板出现文本格式 → 发起 paste 取回内容。
    InitiatePaste(ClipboardFormatId),
    /// 远端请求本地剪贴板数据 → 提交文本响应。
    FormatData(OwnedFormatDataResponse),
    /// 远端文本已取回 → 推给前端。
    RemoteText(String),
}

/// 文本剪贴板后端：只做 CF_UNICODETEXT/CF_TEXT 双向同步，文件复制等
/// 能力不声明、不处理。远端复制文本时主动发起 paste 取回并推给前端；
/// 本地文本由 actor 经共享槽写入，远端请求时应答。
pub(super) struct BridgeClipboardBackend {
    events: tokio::sync::mpsc::UnboundedSender<ClipboardEvent>,
    local_text: Arc<Mutex<Option<String>>>,
    /// 记录最近一次主动 paste 请求的格式，响应用于选择正确的解码方式。
    pending_paste: Option<ClipboardFormatId>,
}

impl BridgeClipboardBackend {
    pub(super) fn new(
        events: tokio::sync::mpsc::UnboundedSender<ClipboardEvent>,
        local_text: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            events,
            local_text,
            pending_paste: None,
        }
    }
}

impl std::fmt::Debug for BridgeClipboardBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BridgeClipboardBackend")
    }
}

ironrdp::core::impl_as_any!(BridgeClipboardBackend);

impl CliprdrBackend for BridgeClipboardBackend {
    fn temporary_directory(&self) -> &str {
        // 文本同步不涉及临时文件。
        ""
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::empty()
    }

    fn on_ready(&mut self) {}

    fn on_request_format_list(&mut self) {
        // 剪贴板内容变化由前端经桥消息驱动，这里不主动采集本地剪贴板。
    }

    fn on_process_negotiated_capabilities(
        &mut self,
        capabilities: ClipboardGeneralCapabilityFlags,
    ) {
        log::debug!(target: "terminal.rdp", "cliprdr negotiated capabilities: {capabilities:?}");
    }

    fn on_remote_copy(&mut self, available_formats: &[ClipboardFormat]) {
        let format = if available_formats
            .iter()
            .any(|format| format.id() == ClipboardFormatId::CF_UNICODETEXT)
        {
            ClipboardFormatId::CF_UNICODETEXT
        } else if available_formats
            .iter()
            .any(|format| format.id() == ClipboardFormatId::CF_TEXT)
        {
            ClipboardFormatId::CF_TEXT
        } else {
            return;
        };
        self.pending_paste = Some(format);
        let _ = self.events.send(ClipboardEvent::InitiatePaste(format));
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        let text = self
            .local_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let response = match (request.format, text) {
            (ClipboardFormatId::CF_UNICODETEXT, Some(text)) => {
                OwnedFormatDataResponse::new_unicode_string(&text)
            }
            (ClipboardFormatId::CF_TEXT, Some(text)) => OwnedFormatDataResponse::new_string(&text),
            _ => OwnedFormatDataResponse::new_error(),
        };
        let _ = self.events.send(ClipboardEvent::FormatData(response));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if response.is_error() {
            // 失败响应同样终结本次 paste；残留 pending 会让下一次响应走错解码路径。
            self.pending_paste.take();
            return;
        }
        let decoded = match self.pending_paste.take() {
            Some(ClipboardFormatId::CF_TEXT) => response.to_string().ok(),
            _ => response.to_unicode_string().ok(),
        };
        if let Some(text) = decoded {
            let _ = self.events.send(ClipboardEvent::RemoteText(text));
        }
    }

    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}

    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, _data_id: LockDataId) {}

    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

pub(super) struct RdpSessionTransport {
    pub listener: TcpListener,
    pub token: String,
    pub framed: RdpFramed,
    pub connection_result: ConnectionResult,
    /// 剪贴板事件接收端。cliprdr SVC 始终随连接注册（见 mod.rs），
    /// `clipboard_sync` 只是前端的运行时门控。
    pub clipboard_events: tokio::sync::mpsc::UnboundedReceiver<ClipboardEvent>,
    /// 本地 → 远端共享文本槽，与 BridgeClipboardBackend 共用。
    pub local_clipboard_text: Arc<Mutex<Option<String>>>,
}

impl SessionTransportRuntime for RdpSessionTransport {
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
            log::debug!(target: "terminal.rdp", "rdp session actor started for session {session_id}");
            run_session_actor(*self, rx, event_tx).await;
        });
    }
}

struct RdpClient {
    ws: WebSocketStream<TcpStream>,
}

async fn run_session_actor(
    transport: RdpSessionTransport,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
) {
    let RdpSessionTransport {
        listener,
        token,
        mut framed,
        connection_result,
        clipboard_events,
        local_clipboard_text,
    } = transport;

    let desktop = connection_result.desktop_size;
    let mut image = DecodedImage::new(PixelFormat::RgbA32, desktop.width, desktop.height);
    let mut active = ActiveStageBuilder {
        static_channels: connection_result.static_channels,
        user_channel_id: connection_result.user_channel_id,
        io_channel_id: connection_result.io_channel_id,
        message_channel_id: connection_result.message_channel_id,
        share_id: connection_result.share_id,
        compression_type: connection_result.compression_type,
        enable_server_pointer: connection_result.enable_server_pointer,
        pointer_software_rendering: connection_result.pointer_software_rendering,
    }
    .build();
    let activation_factory = connection_result.activation_factory;
    let mut input_db = InputDatabase::new();
    let mut client: Option<RdpClient> = None;
    // 动态分辨率请求在 Display Control DVC 握手完成前会被 encode_resize 丢弃；
    // 缓存最近一次请求，每处理完一个服务器 PDU 后重试，保证首次 resize 不丢。
    let mut pending_resize: Option<(u32, u32)> = None;
    // cliprdr SVC 被服务器拒绝时 backend 随之释放、channel 关闭，此后停止轮询。
    let mut clipboard_events = Some(clipboard_events);

    let _ = event_tx.send(SessionWorkerEvent::Ready);

    loop {
        tokio::select! {
            command = rx.recv() => {
                match command {
                    Some(TransportCommand::Close) | None => {
                        // 尽力发 Shutdown Request 让服务器正常注销；失败无所谓，连接随即关闭。
                        if let Ok(outputs) = active.graceful_shutdown() {
                            for output in outputs {
                                if let ActiveStageOutput::ResponseFrame(frame) = output {
                                    let _ = framed.write_all(&frame).await;
                                }
                            }
                        }
                        break;
                    }
                    // 终端写/字符 resize 不适用于图形会话；输入全部走桥协议。
                    Some(_) => {}
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => match accept_bridge_client(stream, &token).await {
                        Ok(ws) => {
                            let mut next = RdpClient { ws };
                            let setup = tokio::time::timeout(
                                BRIDGE_SETUP_TIMEOUT,
                                send_hello_and_full_frame(&mut next, &image),
                            );
                            match setup.await {
                                Ok(Ok(())) => client = Some(next),
                                Ok(Err(error)) => log::warn!(target: "terminal.rdp", "rdp bridge client setup failed: {error}"),
                                Err(_) => log::warn!(target: "terminal.rdp", "rdp bridge client setup timed out"),
                            }
                        }
                        Err(error) => log::warn!(target: "terminal.rdp", "rdp bridge websocket accept failed: {error}"),
                    },
                    Err(error) => {
                        let _ = event_tx.send(SessionWorkerEvent::Failed(format!(
                            "RDP bridge listener failed: {error}"
                        )));
                        break;
                    }
                }
            }
            read = framed.read_pdu() => {
                match read {
                    Ok((action, frame)) => {
                        match active.process(&mut image, action, &frame) {
                            Ok(outputs) => {
                                let mut state = ActorState {
                                    framed: &mut framed,
                                    active: &mut active,
                                    image: &mut image,
                                    input_db: &mut input_db,
                                    local_clipboard_text: &local_clipboard_text,
                                    client: &mut client,
                                    pending_resize: &mut pending_resize,
                                    event_tx: &event_tx,
                                };
                                if !state.handle_outputs(&activation_factory, outputs).await {
                                    break;
                                }
                                // 服务器 PDU 可能完成了 DVC 握手，趁机补发缓存的 resize。
                                if !state.flush_pending_resize().await {
                                    break;
                                }
                            }
                            Err(error) => {
                                let _ = event_tx.send(SessionWorkerEvent::Failed(format!(
                                    "RDP session processing failed: {error}"
                                )));
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                        let _ = event_tx.send(SessionWorkerEvent::Closed(Some(
                            "RDP server closed the connection".to_string(),
                        )));
                        break;
                    }
                    Err(error) => {
                        let _ = event_tx.send(SessionWorkerEvent::Failed(format!(
                            "RDP connection failed: {error}"
                        )));
                        break;
                    }
                }
            }
            message = async { client.as_mut().expect("guarded by precondition").ws.next().await }, if client.is_some() => {
                match message {
                    Some(Ok(Message::Binary(data))) => {
                        match ClientMessage::decode(&data) {
                            Ok(message) => {
                                let mut state = ActorState {
                                    framed: &mut framed,
                                    active: &mut active,
                                    image: &mut image,
                                    input_db: &mut input_db,
                                    local_clipboard_text: &local_clipboard_text,
                                    client: &mut client,
                                    pending_resize: &mut pending_resize,
                                    event_tx: &event_tx,
                                };
                                if !state.handle_client_message(message).await {
                                    break;
                                }
                            }
                            Err(error) => {
                                log::debug!(target: "terminal.rdp", "ignoring malformed bridge message: {error}");
                            }
                        }
                    }
                    // 前端断开/出错：会话继续存活，重连时重发 Hello + 全帧。
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        client = None;
                    }
                    Some(Ok(_)) => {}
                }
            }
            event = async {
                clipboard_events
                    .as_mut()
                    .expect("guarded by precondition")
                    .recv()
                    .await
            }, if clipboard_events.is_some() => {
                match event {
                    Some(event) => {
                        let mut state = ActorState {
                            framed: &mut framed,
                            active: &mut active,
                            image: &mut image,
                            input_db: &mut input_db,
                            local_clipboard_text: &local_clipboard_text,
                            client: &mut client,
                            pending_resize: &mut pending_resize,
                            event_tx: &event_tx,
                        };
                        if !state.handle_clipboard_event(event).await {
                            break;
                        }
                    }
                    // backend 已随会话释放（理论上不会发生，因为 SVC 由 active 持有）。
                    None => {
                        clipboard_events = None;
                    }
                }
            }
        }
    }
    log::debug!(target: "terminal.rdp", "rdp session actor stopped");
}

/// 会话主循环各处理函数共享的可变状态束，避免在长调用链上逐层透传参数。
struct ActorState<'a> {
    framed: &'a mut RdpFramed,
    active: &'a mut ActiveStage,
    image: &'a mut DecodedImage,
    input_db: &'a mut InputDatabase,
    local_clipboard_text: &'a Arc<Mutex<Option<String>>>,
    client: &'a mut Option<RdpClient>,
    pending_resize: &'a mut Option<(u32, u32)>,
    event_tx: &'a tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
}

impl ActorState<'_> {
    /// 上报会话失败并通知主循环退出。会话写失败必须上报：静默退出会让
    /// 前端永远停在已连接状态（worker 持有 event_tx 克隆，channel 不会自闭合）。
    fn fail(&self, message: String) -> bool {
        let _ = self.event_tx.send(SessionWorkerEvent::Failed(message));
        false
    }

    /// 处理一次 `ActiveStage::process` 的输出；返回 false 表示会话应结束。
    async fn handle_outputs(
        &mut self,
        activation_factory: &ConnectionActivationFactory,
        outputs: Vec<ActiveStageOutput>,
    ) -> bool {
        // 每批图形更新打包成一条 Frame 消息下发；桥写失败即丢弃该客户端，
        // 前端重连时会收到 Hello + 全帧。
        let mut dirty_rects: Vec<InclusiveRectangle> = Vec::new();
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(frame) => {
                    if let Err(error) = self.framed.write_all(&frame).await {
                        return self.fail(format!("RDP connection write failed: {error}"));
                    }
                }
                ActiveStageOutput::GraphicsUpdate(rect) => dirty_rects.push(rect),
                ActiveStageOutput::Terminate(reason) => {
                    let description = reason.description();
                    if let Some(active_client) = self.client.as_mut() {
                        let _ = send_message(
                            active_client,
                            &ServerMessage::Disconnect(description.clone()),
                        )
                        .await;
                    }
                    let _ = self
                        .event_tx
                        .send(SessionWorkerEvent::Closed(Some(description)));
                    return false;
                }
                ActiveStageOutput::DeactivateAll => {
                    // 服务器要求重新激活（典型触发：我们发了动态分辨率请求）。
                    match run_deactivation_reactivation(self.framed, activation_factory).await {
                        Ok((desktop_size, share_id)) => {
                            self.active.set_share_id(share_id);
                            if desktop_size.width != self.image.width()
                                || desktop_size.height != self.image.height()
                            {
                                *self.image = DecodedImage::new(
                                    PixelFormat::RgbA32,
                                    desktop_size.width,
                                    desktop_size.height,
                                );
                                if let Some(active_client) = self.client.as_mut() {
                                    if let Err(error) = send_message(
                                        active_client,
                                        &ServerMessage::Resize {
                                            width: desktop_size.width,
                                            height: desktop_size.height,
                                        },
                                    )
                                    .await
                                    {
                                        log::warn!(target: "terminal.rdp", "rdp bridge resize notify failed: {error}");
                                        *self.client = None;
                                    }
                                }
                            }
                            if let Some(active_client) = self.client.as_mut() {
                                if let Err(error) = send_full_frame(active_client, self.image).await
                                {
                                    log::warn!(target: "terminal.rdp", "rdp bridge frame send failed: {error}");
                                    *self.client = None;
                                }
                            }
                        }
                        Err(error) => {
                            return self.fail(error);
                        }
                    }
                }
                // 客户端软件渲染指针已把光标合成进帧缓冲并产出 GraphicsUpdate，
                // 指针类输出无需单独下发；多路传输/自动带宽检测不支持。
                ActiveStageOutput::PointerDefault
                | ActiveStageOutput::PointerHidden
                | ActiveStageOutput::PointerPosition { .. }
                | ActiveStageOutput::PointerBitmap(_)
                | ActiveStageOutput::MultitransportRequest(_)
                | ActiveStageOutput::AutoDetect(_) => {}
            }
        }
        if !dirty_rects.is_empty() {
            if let Some(active_client) = self.client.as_mut() {
                if let Err(error) = send_dirty_rects(active_client, self.image, &dirty_rects).await
                {
                    log::warn!(target: "terminal.rdp", "rdp bridge frame send failed: {error}");
                    *self.client = None;
                }
            }
        }
        true
    }

    /// 处理前端桥消息；返回 false 表示会话应结束。
    async fn handle_client_message(&mut self, message: ClientMessage) -> bool {
        match message {
            ClientMessage::PointerMove { x, y } => {
                self.apply_input(vec![Operation::MouseMove(MousePosition { x, y })])
                    .await
            }
            ClientMessage::PointerButton { button, pressed } => {
                let Some(button) = map_mouse_button(button) else {
                    return true;
                };
                let operation = if pressed {
                    Operation::MouseButtonPressed(button)
                } else {
                    Operation::MouseButtonReleased(button)
                };
                self.apply_input(vec![operation]).await
            }
            ClientMessage::Wheel { delta } => {
                // MS-RDPBCGR 的 WheelRotationMask 以 WHEEL_DELTA(120) 为一格，且是
                // 9-bit 有符号域 [-256, 255]（ironrdp-pdu 编码时校验该范围）。
                let rotation_units = delta.saturating_mul(120).clamp(-256, 255);
                self.apply_input(vec![Operation::WheelRotations(WheelRotations {
                    is_vertical: true,
                    rotation_units,
                })])
                .await
            }
            ClientMessage::Key {
                scancode,
                pressed,
                extended,
            } => {
                // ironrdp 的扩展键约定为 0xE000 前缀位。
                let code = if extended {
                    scancode | 0xE000
                } else {
                    scancode
                };
                let scancode = Scancode::from_u16(code);
                let operation = if pressed {
                    Operation::KeyPressed(scancode)
                } else {
                    Operation::KeyReleased(scancode)
                };
                self.apply_input(vec![operation]).await
            }
            ClientMessage::ResizeRequest { width, height } => {
                *self.pending_resize = Some(MonitorLayoutEntry::adjust_display_size(
                    u32::from(width),
                    u32::from(height),
                ));
                self.flush_pending_resize().await
            }
            ClientMessage::ClipboardText(text) => {
                *self
                    .local_clipboard_text
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text);
                let Some(cliprdr) = self.active.get_svc_processor_mut::<CliprdrClient>() else {
                    return true;
                };
                let messages = match cliprdr
                    .initiate_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)])
                {
                    Ok(messages) => messages,
                    Err(error) => {
                        log::warn!(target: "terminal.rdp", "cliprdr initiate copy failed: {error}");
                        return true;
                    }
                };
                self.write_svc_messages(messages).await
            }
        }
    }

    /// 处理剪贴板 backend 事件；返回 false 表示会话应结束。
    async fn handle_clipboard_event(&mut self, event: ClipboardEvent) -> bool {
        match event {
            ClipboardEvent::InitiatePaste(format) => {
                let Some(cliprdr) = self.active.get_svc_processor_mut::<CliprdrClient>() else {
                    return true;
                };
                match cliprdr.initiate_paste(format) {
                    Ok(messages) => self.write_svc_messages(messages).await,
                    Err(error) => {
                        log::warn!(target: "terminal.rdp", "cliprdr initiate paste failed: {error}");
                        true
                    }
                }
            }
            ClipboardEvent::FormatData(response) => {
                let Some(cliprdr) = self.active.get_svc_processor_mut::<CliprdrClient>() else {
                    return true;
                };
                match cliprdr.submit_format_data(response) {
                    Ok(messages) => self.write_svc_messages(messages).await,
                    Err(error) => {
                        log::warn!(target: "terminal.rdp", "cliprdr submit format data failed: {error}");
                        true
                    }
                }
            }
            ClipboardEvent::RemoteText(text) => {
                if let Some(active_client) = self.client.as_mut() {
                    if let Err(error) =
                        send_message(active_client, &ServerMessage::ClipboardText(text)).await
                    {
                        log::warn!(target: "terminal.rdp", "rdp bridge clipboard send failed: {error}");
                        *self.client = None;
                    }
                }
                true
            }
        }
    }

    async fn write_svc_messages(
        &mut self,
        messages: ironrdp::svc::SvcProcessorMessages<CliprdrClient>,
    ) -> bool {
        let frame = match self.active.process_svc_processor_messages(messages) {
            Ok(frame) => frame,
            Err(error) => {
                log::warn!(target: "terminal.rdp", "cliprdr message encode failed: {error}");
                return true;
            }
        };
        if frame.is_empty() {
            return true;
        }
        match self.framed.write_all(&frame).await {
            Ok(()) => true,
            Err(error) => self.fail(format!("RDP connection write failed: {error}")),
        }
    }

    /// 应用一批输入操作并写回 fast-path 输入帧；指针移动会改动帧缓冲，
    /// 产出的脏矩形随批推给前端。返回 false 表示会话应结束。
    async fn apply_input(&mut self, operations: Vec<Operation>) -> bool {
        let events = self.input_db.apply(operations);
        let outputs = match self.active.process_fastpath_input(self.image, &events) {
            Ok(outputs) => outputs,
            Err(error) => {
                log::warn!(target: "terminal.rdp", "RDP input processing failed: {error}");
                return true;
            }
        };
        let mut dirty_rects: Vec<InclusiveRectangle> = Vec::new();
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(frame) => {
                    if let Err(error) = self.framed.write_all(&frame).await {
                        return self.fail(format!("RDP connection write failed: {error}"));
                    }
                }
                ActiveStageOutput::GraphicsUpdate(rect) => dirty_rects.push(rect),
                _ => {}
            }
        }
        if !dirty_rects.is_empty() {
            if let Some(active_client) = self.client.as_mut() {
                if let Err(error) = send_dirty_rects(active_client, self.image, &dirty_rects).await
                {
                    log::warn!(target: "terminal.rdp", "rdp bridge frame send failed: {error}");
                    *self.client = None;
                }
            }
        }
        true
    }

    /// 尝试下发缓存的动态分辨率请求；Display Control DVC 未就绪时保留 pending
    /// 等待下次重试。返回 false 表示会话应结束。
    async fn flush_pending_resize(&mut self) -> bool {
        let Some((width, height)) = *self.pending_resize else {
            return true;
        };
        match self.active.encode_resize(width, height, None, None) {
            // DVC 未握手完成（或服务器不支持）：保留待重试。
            None => true,
            Some(Ok(bytes)) => {
                *self.pending_resize = None;
                match self.framed.write_all(&bytes).await {
                    Ok(()) => true,
                    Err(error) => self.fail(format!("RDP connection write failed: {error}")),
                }
            }
            // 编码错误不会自愈（尺寸已被 adjust_display_size 钳制），丢弃。
            Some(Err(error)) => {
                log::warn!(target: "terminal.rdp", "RDP resize request encode failed: {error}");
                *self.pending_resize = None;
                true
            }
        }
    }
}

/// Deactivation-Reactivation 序列：服务器接受了新的显示器布局后重新交换
/// 能力并完成 finalize。期间独占 framed，桥消息会短暂阻塞——整个过程通常
/// 只有几个 PDU；对端不应答时超时按会话失败处理，避免 actor 永久挂起。
async fn run_deactivation_reactivation(
    framed: &mut RdpFramed,
    activation_factory: &ConnectionActivationFactory,
) -> Result<(DesktopSize, u32), String> {
    let mut sequence = activation_factory.create();
    let mut buf = WriteBuf::new();
    let run = async {
        loop {
            ironrdp_tokio::single_sequence_step(framed, &mut sequence, &mut buf)
                .await
                .map_err(|error| {
                    format!("RDP deactivation-reactivation sequence failed: {error}")
                })?;
            if let ConnectionActivationState::Finalized {
                desktop_size,
                share_id,
                ..
            } = sequence.connection_activation_state()
            {
                return Ok((desktop_size, share_id));
            }
        }
    };
    tokio::time::timeout(REACTIVATION_TIMEOUT, run)
        .await
        .map_err(|_| "RDP deactivation-reactivation sequence timed out".to_string())?
}

async fn send_hello_and_full_frame(
    client: &mut RdpClient,
    image: &DecodedImage,
) -> Result<(), String> {
    send_message(
        client,
        &ServerMessage::Hello {
            width: image.width(),
            height: image.height(),
        },
    )
    .await?;
    send_full_frame(client, image).await
}

async fn send_full_frame(client: &mut RdpClient, image: &DecodedImage) -> Result<(), String> {
    if image.width() == 0 || image.height() == 0 {
        return Ok(());
    }
    // 全帧就是一个覆盖整个画面的脏矩形。
    let full = InclusiveRectangle {
        left: 0,
        top: 0,
        right: image.width() - 1,
        bottom: image.height() - 1,
    };
    send_dirty_rects(client, image, &[full]).await
}

/// 把脏矩形从帧缓冲中逐行拷贝成紧凑 RGBA 打包下发。越界矩形（例如 resize
/// 竞态期间的旧坐标）跳过而不是失败。
async fn send_dirty_rects(
    client: &mut RdpClient,
    image: &DecodedImage,
    rects: &[InclusiveRectangle],
) -> Result<(), String> {
    let stride = image.stride();
    let bpp = image.bytes_per_pixel();
    let mut packed = Vec::with_capacity(rects.len());
    for rect in rects {
        if rect.right >= image.width() || rect.bottom >= image.height() {
            continue;
        }
        let width = rect.width();
        let height = rect.height();
        let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * bpp);
        for row in rect.top..=rect.bottom {
            let start = usize::from(row) * stride + usize::from(rect.left) * bpp;
            let end = start + usize::from(width) * bpp;
            pixels.extend_from_slice(&image.data()[start..end]);
        }
        packed.push(DirtyRect {
            x: rect.left,
            y: rect.top,
            width,
            height,
            pixels,
        });
    }
    if packed.is_empty() {
        return Ok(());
    }
    send_message(client, &ServerMessage::Frame { rects: packed }).await
}

async fn send_message(client: &mut RdpClient, message: &ServerMessage) -> Result<(), String> {
    client
        .ws
        .send(Message::Binary(Bytes::from(message.encode())))
        .await
        .map_err(|error| format!("websocket send failed: {error}"))
}

fn map_mouse_button(button: u8) -> Option<MouseButton> {
    match button {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Right),
        2 => Some(MouseButton::Middle),
        _ => None,
    }
}
