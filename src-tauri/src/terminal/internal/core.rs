use crate::{
    state::{AppState, ConnectionOpenScope},
    terminal::api::dto::SshCredentialOverride,
    terminal::domain::ProtocolKind,
    workspace::{workspace_connection_by_id, ConnectionProfile, JumpHostHop},
};
use bytes::Bytes;
use encoding_rs::Encoding;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tokio_serial::{SerialPort, SerialPortBuilderExt};

use super::ssh_client::SharedSshSession;

pub(super) const SESSION_BUFFER_SIZE: usize = 64 * 1024;
pub(super) const CONNECT_TIMEOUT_MS: u64 = 8_000;
pub(super) const STARTUP_AUTH_FOLLOWUP_TIMEOUT_MS: u64 = 5_000;
pub(super) const SERIAL_WRITE_STALL_TIMEOUT_MS: u64 = 3_000;
pub(super) const SERIAL_READ_BATCH_MAX_BYTES: usize = 32 * 1024;
pub(super) const OUTPUT_FLUSH_MAX_BYTES: usize = 128 * 1024;
pub(super) const OUTPUT_LIVE_FLUSH_WINDOW_MS: u64 = 4;
pub(super) const OUTPUT_LIVE_FLUSH_MIN_BYTES: usize = 32 * 1024;
pub(super) const SFTP_TRANSFER_BUFFER_BYTES: usize = 256 * 1024;
pub(super) const CODEC_RAW_BUFFER_CAPACITY: usize = 4 * 1024;
pub(super) const DETECTION_SAMPLE_MAX_BYTES: usize = 4096;
pub(super) const DETECTION_LOCK_CONFIDENCE: f32 = 0.95;
pub(super) const DETECTION_ERROR_UNLOCK_THRESHOLD: u8 = 3;
pub(super) const SERIAL_FAST_BAUD_SAMPLE_MS: u64 = 220;
pub(super) const SERIAL_PASSIVE_BAUD_SAMPLE_MS: u64 = 80;
pub(super) const SERIAL_SAMPLE_MAX_BYTES: usize = 8192;
pub(super) const SERIAL_MIN_DETECT_BYTES: usize = 4;
pub(super) const SERIAL_PROBE_SETTLE_MS: u64 = 15;
pub(super) const SERIAL_PROBE_INTER_BYTE_TIMEOUT_MS: u64 = 90;
pub(super) const SERIAL_PROBE_MAX_SAMPLE_MS: u64 = 520;
pub(super) const SERIAL_RELIABLE_BAUD_SCORE: f32 = 0.52;
pub(super) const SERIAL_WAKE_SEQUENCE: &[u8] = b"\r";
pub(super) const SERIAL_FALLBACK_BAUD_RATE: u32 = 9_600;
pub(super) const SERIAL_QUICK_AUTO_BAUD_CANDIDATES: &[u32] = &[9_600, 115_200];
pub(super) const BAUD_CANDIDATES: &[u32] = &[
    9_600, 115_200, 38_400, 19_200, 57_600, 4_800, 2_400, 1_200, 300, 14_400, 230_400, 460_800,
    921_600, 1_000_000, 2_000_000,
];

/// Maximum bytes retained in the terminal replay cache. Older output is trimmed
/// to keep reattached frontends responsive and bound backend memory.
pub(super) const REPLAY_CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Maximum bytes the frontend may be behind before live output is paused.
/// Combined with the rendered-offset report, this provides backpressure against
/// a slow renderer without dropping data.
pub(super) const MAX_UNRENDERED_BYTES: usize = 256 * 1024;

/// Grace period before the render gate fails open if the frontend never reports
/// a rendered offset. Prevents legacy or broken frontends from deadlocking.
pub(super) const RENDER_GATE_FAIL_OPEN_MS: u64 = 500;

/// Retry interval when live output is blocked waiting for the frontend renderer.
pub(super) const RENDER_GATE_RETRY_MS: u64 = 16;

/// Time allowed for a transport write to make progress before it is considered
/// stalled and the session is failed.
pub(super) const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) type ConnectionResult<T> = Result<T, ConnectionError>;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionOpenRequest {
    pub(crate) connection_id: String,
    pub(crate) open_request_id: Option<String>,
    pub(crate) trust_host_key: Option<bool>,
    pub(crate) accept_host_key_once: Option<bool>,
    pub(crate) terminal_scrollback: Option<u32>,
    pub(crate) terminal_type: Option<String>,
    pub(crate) encoding: Option<String>,
    pub(crate) realtime_encoding_detection: Option<bool>,
    pub(crate) cols: Option<u32>,
    pub(crate) rows: Option<u32>,
    pub(crate) ssh_credential: Option<SshCredentialOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyDeleteRequest {
    pub(super) connection_id: String,
}

#[derive(Debug, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum ConnectionOpenResult {
    Connected {
        session_id: String,
        protocol: String,
        encoding: String,
        serial_port: Option<String>,
        baud_rate: Option<u32>,
        serial_scores: Option<Vec<SerialProbeResult>>,
    },
    HostKeyPrompt {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
}

impl ConnectionOpenResult {
    pub(super) fn connected_shell(
        session_id: String,
        protocol: ProtocolKind,
        encoding: String,
    ) -> Self {
        Self::Connected {
            session_id,
            protocol: protocol.as_str().to_string(),
            encoding,
            serial_port: None,
            baud_rate: None,
            serial_scores: None,
        }
    }

    pub(super) fn connected_serial(
        session_id: String,
        encoding: String,
        serial_port: String,
        baud_rate: u32,
        serial_scores: Vec<SerialProbeResult>,
    ) -> Self {
        Self::Connected {
            session_id,
            protocol: ProtocolKind::Serial.as_str().to_string(),
            encoding,
            serial_port: Some(serial_port),
            baud_rate: Some(baud_rate),
            serial_scores: Some(serial_scores),
        }
    }
}

#[derive(Clone, Debug, Error, Serialize)]
#[error("{code}: {detail}")]
#[serde(rename_all = "camelCase")]
pub struct ConnectionError {
    pub(crate) code: &'static str,
    pub(crate) detail: String,
    pub(crate) retryable: bool,
    /// Structured arguments that the frontend can use to localize the error
    /// message. Kept separate from `detail` so English debug details do not
    /// pollute translated user-facing strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) args: Option<serde_json::Value>,
}

impl ConnectionError {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            detail: detail.into(),
            retryable,
            args: None,
        }
    }

    pub(crate) fn with_args(
        code: &'static str,
        detail: impl Into<String>,
        args: impl Into<serde_json::Value>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            detail: detail.into(),
            retryable,
            args: Some(args.into()),
        }
    }

    pub(crate) fn validation(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(code, detail, false)
    }

    pub(crate) fn cancelled() -> Self {
        Self::new(
            "connection_open_cancelled",
            "connection open was cancelled",
            false,
        )
    }

    pub(crate) fn connection_not_active(detail: impl Into<String>) -> Self {
        Self::new("connection_not_active", detail, false)
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.code == "connection_open_cancelled"
    }

    pub(crate) fn serial_port_not_found(port_name: &str, detail: String) -> Self {
        Self::with_args(
            "serial_port_not_found",
            format!("port={port_name}; {detail}"),
            serde_json::json!({ "portName": port_name, "detail": detail }),
            true,
        )
    }

    pub(crate) fn serial_port_unavailable(port_name: &str, detail: String) -> Self {
        Self::with_args(
            "serial_port_unavailable",
            format!("port={port_name}; {detail}"),
            serde_json::json!({ "portName": port_name, "detail": detail }),
            true,
        )
    }

    pub(crate) fn internal(error: String) -> Self {
        Self::new("internal_error", error, true)
    }
}

impl From<String> for ConnectionError {
    fn from(value: String) -> Self {
        Self::internal(value)
    }
}

impl From<&str> for ConnectionError {
    fn from(value: &str) -> Self {
        Self::internal(value.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionReadResult {
    pub(super) data: String,
    pub(super) raw_bytes: Vec<u8>,
    pub(super) encoding: String,
    pub(super) confidence: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWorkingDirectoryEvent {
    pub(super) session_id: String,
    pub(super) path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortOption {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) label: String,
}

/// 串口名的自然序比较:`ttyUSB2` 排在 `ttyUSB10` 之前(数字段按数值比较)。
/// 供探测顺序与列表展示共用,保证两处排序一致。
pub(crate) fn compare_serial_port_names(a: &str, b: &str) -> std::cmp::Ordering {
    serial_port_sort_key(a)
        .cmp(&serial_port_sort_key(b))
        .then_with(|| a.cmp(b))
}

pub(crate) fn serial_port_sort_key(port: &str) -> (String, u32) {
    let trimmed = port.trim();
    let split_at = trimmed
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let (prefix, digits) = trimmed.split_at(split_at);
    let number = digits.parse::<u32>().unwrap_or(u32::MAX);
    (prefix.to_ascii_lowercase(), number)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SshRuntimeMetricsRequest {
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SshRuntimeMetrics {
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    pub(super) cpu_percent: f32,
    pub(super) cpu_user_percent: Option<f32>,
    pub(super) cpu_system_percent: Option<f32>,
    pub(super) cpu_iowait_percent: Option<f32>,
    pub(super) cpu_steal_percent: Option<f32>,
    pub(super) memory_percent: f32,
    pub(super) disk_percent: f32,
    pub(super) load_average: Option<String>,
    pub(super) latency_ms: Option<f32>,
    pub(super) sample_timestamp_ms: u64,
    pub(super) unavailable: bool,
    pub(super) cpu_ready: bool,
    pub(super) memory_total: Option<u64>,
    pub(super) memory_used: Option<u64>,
    pub(super) memory_available: Option<u64>,
    pub(super) swap_total: Option<u64>,
    pub(super) swap_used: Option<u64>,
    pub(super) swap_percent: Option<f32>,
    pub(super) disk_total: Option<u64>,
    pub(super) disk_used: Option<u64>,
    pub(super) disk_available: Option<u64>,
    pub(super) disk_inode_percent: Option<f32>,
    pub(super) network_rx_rate: Option<f32>,
    pub(super) network_tx_rate: Option<f32>,
    pub(super) process_count: Option<u64>,
    pub(super) thread_count: Option<u64>,
    pub(super) uptime_seconds: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_total: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_idle: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_user: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_system: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_iowait: Option<u64>,
    #[serde(skip)]
    pub(super) cpu_steal: Option<u64>,
    #[serde(skip)]
    pub(super) network_rx_bytes: Option<u64>,
    #[serde(skip)]
    pub(super) network_tx_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpListRemoteRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpListResult {
    pub(super) path: String,
    pub(super) parent: Option<String>,
    pub(super) entries: Vec<SftpEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpEntry {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) kind: String,
    pub(super) size: u64,
    pub(super) modified: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpFileStatResult {
    pub(super) path: String,
    pub(super) kind: String,
    pub(super) size: u64,
    pub(super) modified: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpCloseSessionRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpTransferRequest {
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    #[serde(default)]
    pub(crate) transfer_id: Option<String>,
    pub(crate) direction: String,
    pub(crate) local_path: String,
    pub(crate) remote_path: Option<String>,
    pub(crate) remote_parent_path: Option<String>,
    pub(crate) remote_name: Option<String>,
    #[serde(default)]
    pub(crate) upload_conflict_action: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpTransferControlRequest {
    pub(super) transfer_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpTransferItem {
    pub(super) transfer_id: String,
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) direction: String,
    pub(super) name: String,
    pub(super) transferred: u64,
    pub(super) total: u64,
    pub(super) status: String,
    pub(super) error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpChooseDownloadPathRequest {
    pub(super) default_file_name: String,
    pub(super) kind: String,
    pub(super) title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpChooseUploadFilesRequest {
    pub(super) title: Option<String>,
    pub(super) all_files_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrzszChooseUploadFilesRequest {
    #[serde(default)]
    pub(super) directory: bool,
    pub(super) title: Option<String>,
    pub(super) all_files_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrzszChooseDownloadDirectoryRequest {
    pub(super) title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpDeleteRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpCreateDirRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) parent_path: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpCreateFileRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) parent_path: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpReadFileRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpWriteFileRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) path: String,
    pub(super) content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpStatFileRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SftpRenameRequest {
    pub(super) connection_id: String,
    pub(super) session_id: String,
    pub(super) from_path: String,
    pub(super) to_parent_path: String,
    pub(super) to_name: String,
    #[serde(default)]
    pub(super) conflict_action: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SftpTransferProgressEvent {
    pub(crate) transfer_id: String,
    pub(crate) transferred: u64,
    pub(crate) total: u64,
    pub(crate) done: bool,
    pub(crate) status: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialProbeResult {
    pub(super) port_name: String,
    pub(super) baud_rate: u32,
    pub(super) score: f32,
    pub(super) bytes_read: usize,
    pub(super) can_open: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialRedetectResult {
    pub(super) serial_port: String,
    pub(super) baud_rate: u32,
    pub(super) confirmed: bool,
    pub(super) serial_scores: Vec<SerialProbeResult>,
    #[serde(skip)]
    pub(super) initial_sample: Vec<u8>,
}

#[derive(Clone)]
pub struct TerminalSession {
    pub(crate) tx: tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    pub(crate) capabilities: crate::terminal::domain::ConnectionCapabilities,
    pub(crate) resources: TerminalSessionResources,
    /// Handle to the async worker task driving this session. Stored so callers
    /// can observe task completion and enforce bounded shutdown waits.
    pub(crate) worker: std::sync::Arc<tokio::task::JoinHandle<()>>,
}

#[derive(Clone, Default)]
pub struct TerminalSessionResources {
    pub(crate) ssh: Option<SshSessionResources>,
    pub(crate) serial: Option<SerialSessionResources>,
    pub(crate) disposables: Vec<SessionDisposableResource>,
}

#[derive(Clone)]
pub struct SshSessionResources {
    pub(crate) session: SharedSshSession,
}

#[derive(Clone)]
pub struct SerialSessionResources {
    pub(crate) port_name: String,
}

#[derive(Clone)]
pub enum SessionDisposableResource {
    SshChain { sessions: Vec<SharedSshSession> },
}

impl SessionDisposableResource {
    pub(crate) fn dispose(&self) {
        match self {
            SessionDisposableResource::SshChain { sessions } => {
                for jump_session in sessions {
                    let jump_session = jump_session.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = jump_session
                            .lock()
                            .await
                            .disconnect(russh::Disconnect::ByApplication, "connection closed", "")
                            .await;
                    });
                }
            }
        }
    }
}

impl TerminalSessionResources {
    pub(crate) fn ssh(session: SharedSshSession, chain_sessions: Vec<SharedSshSession>) -> Self {
        Self {
            ssh: Some(SshSessionResources { session }),
            disposables: vec![SessionDisposableResource::SshChain {
                sessions: chain_sessions,
            }],
            ..Self::default()
        }
    }

    pub(crate) fn serial(port_name: String) -> Self {
        Self {
            serial: Some(SerialSessionResources { port_name }),
            ..Self::default()
        }
    }

    pub(crate) fn ssh_aux_session(&self) -> Option<SharedSshSession> {
        self.ssh.as_ref().map(|resources| resources.session.clone())
    }

    pub(crate) fn dispose(&self) {
        for resource in &self.disposables {
            resource.dispose();
        }
    }

    pub(crate) fn serial_port(&self) -> Option<&str> {
        self.serial
            .as_ref()
            .map(|resources| resources.port_name.as_str())
    }
}

pub(crate) enum SessionCommand {
    Activate {
        channel_id: u64,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Deactivate {
        channel_id: Option<u64>,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Write {
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data: String,
    },
    WriteBytes {
        channel_id: Option<u64>,
        input_sequence: Option<u64>,
        data: Vec<u8>,
    },
    FlushOutput,
    /// Report from the frontend describing how much output has been rendered.
    /// Used to apply backpressure so a slow renderer cannot be flooded.
    RenderedOffset {
        channel_id: u64,
        offset: u64,
    },
    SetEncodingDetection {
        channel_id: Option<u64>,
        enabled: bool,
        encoding: Option<String>,
    },
    SetRawOutput {
        channel_id: Option<u64>,
        enabled: bool,
    },
    InvokeCapability(SessionCapabilityCommand),
    Resize {
        channel_id: Option<u64>,
        cols: u32,
        rows: u32,
        width_px: Option<u32>,
        height_px: Option<u32>,
    },
    Close,
}

pub(super) type SessionTransport = Box<dyn SessionTransportRuntime>;

pub(crate) enum SessionCapabilityCommand {
    RedetectSerialBaud {
        reply: tokio::sync::oneshot::Sender<Result<SerialRedetectResult, String>>,
    },
}

pub(super) enum SessionWorkerEvent {
    Ready,
    Data {
        bytes: Bytes,
        negotiated_encoding: Option<String>,
    },
    Closed(Option<String>),
    Failed(String),
}

pub(super) enum TransportCommand {
    Write(Vec<u8>),
    InvokeCapability(TransportCapabilityCommand),
    Resize(TerminalResize),
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TransportCommandOutcome {
    Continue,
    Close,
}

pub(super) enum TransportCapabilityCommand {
    RedetectSerialBaud {
        encoding: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<SerialRedetectResult, String>>,
    },
}

pub(super) trait SessionTransportRuntime: Send {
    fn initial_size(&self) -> Option<TerminalSize>;

    fn supports_raw_bytes(&self) -> bool {
        false
    }

    fn spawn(
        self: Box<Self>,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
        event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TerminalSize {
    pub(super) cols: u32,
    pub(super) rows: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TerminalResize {
    pub(super) cols: u32,
    pub(super) rows: u32,
    pub(super) width_px: Option<u32>,
    pub(super) height_px: Option<u32>,
}

impl TerminalResize {
    pub(super) fn size(self) -> TerminalSize {
        TerminalSize {
            cols: self.cols,
            rows: self.rows,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TelnetConfig {
    pub(super) terminal_type: String,
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) env_vars: Vec<(TelnetEnvVarKind, String, String)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TelnetEnvVarKind {
    WellKnown,
    UserDefined,
}

#[derive(Clone)]
pub struct CodecState {
    pub(super) backend_encoding: Option<String>,
    pub(super) backend_encoding_handle: Option<&'static Encoding>,
    pub(super) detected_encoding: Option<String>,
    pub(super) detected_encoding_handle: Option<&'static Encoding>,
    pub(super) detected_confidence: f32,
    pub(super) consecutive_errors: u8,
    pub(super) raw_buffer: Vec<u8>,
    pub(super) pending_tail: Vec<u8>,
    pub(super) realtime_detection_enabled: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct SerialLineSettings {
    pub(super) data_bits: tokio_serial::DataBits,
    pub(super) flow_control: tokio_serial::FlowControl,
    pub(super) parity: tokio_serial::Parity,
    pub(super) stop_bits: tokio_serial::StopBits,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedConnection {
    pub(crate) id: String,
    pub(crate) open_request_id: Option<String>,
    pub(crate) open_scope: Option<ConnectionOpenScope>,
    pub(crate) protocol: ProtocolKind,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) user: Option<String>,
    pub(crate) serial_port: Option<String>,
    pub(crate) baud_rate: Option<u32>,
    pub(crate) serial_quick_auto_baud: Option<bool>,
    pub(crate) data_bits: Option<u8>,
    pub(crate) flow_control: Option<String>,
    pub(crate) parity: Option<String>,
    pub(crate) stop_bits: Option<u8>,
    pub(crate) encoding: Option<String>,
    pub(crate) realtime_encoding_detection: Option<bool>,
    pub(crate) auth_method: Option<String>,
    pub(crate) saved_credential_id: Option<String>,
    /// Inline password from a deep-link URI (`ssh://user:password@host`).
    /// Takes precedence over `saved_credential_id` when present.
    pub(crate) inline_password: Option<String>,
    /// Inline private key from a transient resolved connection.
    pub(crate) inline_private_key: Option<String>,
    /// Passphrase for an inline private key.
    pub(crate) inline_private_key_passphrase: Option<String>,
    pub(crate) trust_host_key: Option<bool>,
    pub(crate) accept_host_key_once: Option<bool>,
    pub(crate) terminal_scrollback: Option<u32>,
    pub(crate) terminal_type: Option<String>,
    pub(crate) cols: Option<u32>,
    pub(crate) rows: Option<u32>,
    pub(crate) jump_hosts: Option<Vec<JumpHostHop>>,
}

pub(crate) struct SessionOpenContext {
    pub(crate) connection_id: String,
    pub(crate) encoding_label: String,
    pub(crate) codec: CodecState,
    pub(crate) replay_line_limit: usize,
}

impl ResolvedConnection {
    pub(crate) fn session_open_context(&self, state: &AppState) -> SessionOpenContext {
        let encoding = self.encoding.clone();
        SessionOpenContext {
            connection_id: self.id.clone(),
            encoding_label: encoding.clone().unwrap_or_else(|| "auto".to_string()),
            codec: CodecState::new(encoding, self.realtime_encoding_detection.unwrap_or(true)),
            replay_line_limit: resolve_terminal_replay_line_limit(state, self.terminal_scrollback),
        }
    }

    pub(crate) fn from_profile(
        profile: ConnectionProfile,
        request: ConnectionOpenRequest,
    ) -> Result<Self, ConnectionError> {
        let protocol_kind = ProtocolKind::from_str(&profile.protocol).ok_or_else(|| {
            ConnectionError::validation(
                "unsupported_connection_protocol",
                format!(
                    "unsupported connection protocol '{}'",
                    profile.protocol.trim()
                ),
            )
        })?;
        let is_serial = protocol_kind == ProtocolKind::Serial;
        let port_str = profile.port.as_deref().unwrap_or("");
        let numeric_port: Option<u16> = port_str.parse().ok();
        Ok(Self {
            id: profile.id.clone(),
            open_request_id: request.open_request_id,
            open_scope: None,
            protocol: protocol_kind,
            host: if is_serial {
                Some(port_str.to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| Some(profile.host.clone()))
            } else {
                Some(profile.host.clone())
            },
            port: numeric_port,
            user: Some(profile.user.clone()).filter(|value| !value.trim().is_empty()),
            serial_port: if is_serial {
                profile.port.clone()
            } else {
                None
            },
            baud_rate: profile.baud_rate().and_then(|value| match value {
                crate::workspace::ConnectionBaudRate::Rate(rate) => Some(*rate),
                crate::workspace::ConnectionBaudRate::Auto(_) => None,
            }),
            serial_quick_auto_baud: profile.serial_quick_auto_baud(),
            data_bits: if is_serial { profile.data_bits() } else { None },
            flow_control: if is_serial {
                profile.flow_control().map(str::to_string)
            } else {
                None
            },
            parity: if is_serial {
                profile.parity().map(str::to_string)
            } else {
                None
            },
            stop_bits: if is_serial { profile.stop_bits() } else { None },
            encoding: profile.options.encoding.clone(),
            realtime_encoding_detection: profile
                .options
                .realtime_encoding_detection
                .or(Some(profile.options.encoding.is_none())),
            auth_method: profile.auth_method().map(str::to_string),
            saved_credential_id: profile.saved_credential_id().map(str::to_string),
            inline_password: None,
            inline_private_key: None,
            inline_private_key_passphrase: None,
            trust_host_key: request.trust_host_key,
            accept_host_key_once: request.accept_host_key_once,
            terminal_scrollback: request.terminal_scrollback,
            terminal_type: profile
                .options
                .terminal_type
                .clone()
                .or(request.terminal_type),
            cols: request.cols,
            rows: request.rows,
            jump_hosts: profile.jump_hosts().cloned(),
        }
        .with_ssh_credential_override(request.ssh_credential))
    }

    pub(crate) fn with_open_request(mut self, request: ConnectionOpenRequest) -> Self {
        self.open_request_id = request.open_request_id;
        self.trust_host_key = request.trust_host_key;
        self.accept_host_key_once = request.accept_host_key_once;
        self.terminal_scrollback = request.terminal_scrollback;
        self.terminal_type = request.terminal_type;
        self.encoding = request.encoding;
        self.realtime_encoding_detection = request.realtime_encoding_detection;
        self.cols = request.cols;
        self.rows = request.rows;
        self.apply_ssh_credential_override(request.ssh_credential);
        self
    }

    pub(crate) fn with_open_scope(mut self, scope: ConnectionOpenScope) -> Self {
        self.open_scope = Some(scope);
        self
    }

    fn with_ssh_credential_override(mut self, credential: Option<SshCredentialOverride>) -> Self {
        self.apply_ssh_credential_override(credential);
        self
    }

    fn apply_ssh_credential_override(&mut self, credential: Option<SshCredentialOverride>) {
        let Some(credential) = credential else {
            return;
        };
        self.inline_password = None;
        self.inline_private_key = None;
        self.inline_private_key_passphrase = None;
        match credential {
            SshCredentialOverride::Password { username, password } => {
                self.auth_method = Some("password".to_string());
                if let Some(username) = username.filter(|value| !value.trim().is_empty()) {
                    self.user = Some(username);
                }
                self.inline_password = Some(password);
            }
            SshCredentialOverride::Key {
                username,
                private_key,
                passphrase,
            } => {
                self.auth_method = Some("key".to_string());
                if let Some(username) = username.filter(|value| !value.trim().is_empty()) {
                    self.user = Some(username);
                }
                self.inline_private_key = Some(private_key);
                self.inline_private_key_passphrase = passphrase;
            }
        }
    }
}

pub(super) fn resolve_terminal_replay_line_limit(
    state: &AppState,
    requested_scrollback: Option<u32>,
) -> usize {
    let configured = requested_scrollback
        .map(|value| value as usize)
        .or_else(|| {
            state
                .store()
                .preferences()
                .ok()
                .and_then(|preferences| usize::try_from(preferences.terminal_scrollback).ok())
        })
        .unwrap_or(9001);
    configured.max(1)
}

fn resolve_workspace_connection(
    state: &AppState,
    request: ConnectionOpenRequest,
) -> ConnectionResult<ResolvedConnection> {
    let connection_id = request.connection_id.trim();
    if connection_id.is_empty() {
        return Err(ConnectionError::validation(
            "connection_id_required",
            "connection id is required",
        ));
    }
    let profile = workspace_connection_by_id(state, connection_id)?.ok_or_else(|| {
        ConnectionError::validation(
            "connection_profile_not_found",
            "connection profile was not found",
        )
    })?;
    ResolvedConnection::from_profile(profile, request)
}

pub(crate) fn resolve_connection_request(
    state: &AppState,
    request: ConnectionOpenRequest,
) -> ConnectionResult<ResolvedConnection> {
    let connection_id = request.connection_id.trim();
    if connection_id.is_empty() {
        return Err(ConnectionError::validation(
            "connection_id_required",
            "connection id is required",
        ));
    }
    if let Some(transient) = state.transient_connection(connection_id) {
        return Ok(transient.with_open_request(request));
    }
    resolve_workspace_connection(state, request)
}

impl SerialLineSettings {
    pub(crate) fn from_open_request(request: &ResolvedConnection) -> ConnectionResult<Self> {
        Self::from_parts(
            request.data_bits,
            request.flow_control.as_deref(),
            request.parity.as_deref(),
            request.stop_bits,
        )
        .map_err(|error| ConnectionError::validation("serial_line_settings_invalid", error))
    }

    pub(crate) fn from_parts(
        data_bits: Option<u8>,
        flow_control: Option<&str>,
        parity: Option<&str>,
        stop_bits: Option<u8>,
    ) -> Result<Self, String> {
        Ok(Self {
            data_bits: parse_serial_data_bits(data_bits)?,
            flow_control: parse_serial_flow_control(flow_control)?,
            parity: parse_serial_parity(parity)?,
            stop_bits: parse_serial_stop_bits(stop_bits)?,
        })
    }
}

pub(crate) fn parse_serial_data_bits(value: Option<u8>) -> Result<tokio_serial::DataBits, String> {
    match value.unwrap_or(8) {
        5 => Ok(tokio_serial::DataBits::Five),
        6 => Ok(tokio_serial::DataBits::Six),
        7 => Ok(tokio_serial::DataBits::Seven),
        8 => Ok(tokio_serial::DataBits::Eight),
        value => Err(format!("unsupported serial data bits '{value}'")),
    }
}

pub(crate) fn parse_serial_flow_control(
    value: Option<&str>,
) -> Result<tokio_serial::FlowControl, String> {
    match value.unwrap_or("none").trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(tokio_serial::FlowControl::None),
        "software" | "xonxoff" | "xon-xoff" => Ok(tokio_serial::FlowControl::Software),
        "hardware" | "rtscts" | "rts-cts" => Ok(tokio_serial::FlowControl::Hardware),
        value => Err(format!("unsupported serial flow control '{value}'")),
    }
}

pub(crate) fn parse_serial_parity(value: Option<&str>) -> Result<tokio_serial::Parity, String> {
    match value.unwrap_or("none").trim().to_ascii_lowercase().as_str() {
        "" | "none" | "n" => Ok(tokio_serial::Parity::None),
        "odd" | "o" => Ok(tokio_serial::Parity::Odd),
        "even" | "e" => Ok(tokio_serial::Parity::Even),
        value => Err(format!("unsupported serial parity '{value}'")),
    }
}

pub(crate) fn parse_serial_stop_bits(value: Option<u8>) -> Result<tokio_serial::StopBits, String> {
    match value.unwrap_or(1) {
        1 => Ok(tokio_serial::StopBits::One),
        2 => Ok(tokio_serial::StopBits::Two),
        value => Err(format!("unsupported serial stop bits '{value}'")),
    }
}

pub(crate) fn open_serial_port(
    port_name: &str,
    baud_rate: u32,
    settings: &SerialLineSettings,
) -> tokio_serial::Result<tokio_serial::SerialStream> {
    let mut port = tokio_serial::new(port_name, baud_rate)
        .data_bits(settings.data_bits)
        .flow_control(settings.flow_control)
        .parity(settings.parity)
        .stop_bits(settings.stop_bits)
        .preserve_dtr_on_open()
        .open_native_async()?;

    prime_serial_control_lines(port_name, settings, &mut port);
    Ok(port)
}

fn prime_serial_control_lines(
    port_name: &str,
    settings: &SerialLineSettings,
    port: &mut tokio_serial::SerialStream,
) {
    // Match common terminal programs: many USB serial devices only start output
    // after DTR, and sometimes RTS, are asserted.
    if let Err(error) = port.write_data_terminal_ready(true) {
        log::debug!(target: "terminal.core", "failed to assert DTR on serial port '{port_name}': {error}");
    }

    if matches!(settings.flow_control, tokio_serial::FlowControl::Hardware) {
        return;
    }

    if let Err(error) = port.write_request_to_send(true) {
        log::debug!(target: "terminal.core", "failed to assert RTS on serial port '{port_name}': {error}");
    }
}
