use std::collections::HashSet;
use std::io::ErrorKind;
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use tauri::AppHandle;
use tokio_serial::SerialPort;

use crate::{
    state::AppState,
    terminal::internal::{
        codec::analyze_serial_sample,
        core::{
            compare_serial_port_names, open_serial_port, ConnectionError, ConnectionOpenResult,
            ConnectionResult, ResolvedConnection, SerialLineSettings, SerialProbeResult,
            SerialRedetectResult, SessionCommand, SessionTransportRuntime, SessionWorkerEvent,
            TerminalSession, TerminalSessionResources, TerminalSize, TransportCommand,
            BAUD_CANDIDATES, SERIAL_FALLBACK_BAUD_RATE, SERIAL_FAST_BAUD_SAMPLE_MS,
            SERIAL_MIN_DETECT_BYTES, SERIAL_PASSIVE_BAUD_SAMPLE_MS,
            SERIAL_PROBE_INTER_BYTE_TIMEOUT_MS, SERIAL_PROBE_MAX_SAMPLE_MS, SERIAL_PROBE_SETTLE_MS,
            SERIAL_QUICK_AUTO_BAUD_CANDIDATES, SERIAL_RELIABLE_BAUD_SCORE, SERIAL_SAMPLE_MAX_BYTES,
            SERIAL_WAKE_SEQUENCE, SESSION_BUFFER_SIZE,
        },
        serial_transport::spawn_serial_transport_actor,
        startup_auth::resolve_startup_password_auth,
        terminal::{spawn_bound_session, BoundSessionOptions},
        util::{cancelable_open, ensure_open_current, ensure_open_not_cancelled},
    },
};

/// A serial port candidate for auto detection. `usb` marks USB-to-serial
/// converters, which are probed first: onboard/PCI ports are frequently
/// openable but have no device attached, and scanning them wastes seconds.
#[derive(Clone)]
pub(crate) struct SerialPortCandidate {
    pub(crate) name: String,
    pub(crate) usb: bool,
}

struct SerialOpenSelection {
    port_name: String,
    baud_rate: u32,
    scores: Vec<SerialProbeScore>,
    initial_sample: Vec<u8>,
    port: tokio_serial::SerialStream,
}

pub(super) struct SerialSessionTransport {
    pub(super) port: tokio_serial::SerialStream,
    pub(super) port_name: String,
    /// 传输 actor 退出并释放串口 fd 后通知;重开端口前必须等到它。
    pub(super) close_ack: tokio::sync::oneshot::Sender<()>,
}

impl SessionTransportRuntime for SerialSessionTransport {
    fn initial_size(&self) -> Option<TerminalSize> {
        None
    }

    fn spawn(
        self: Box<Self>,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
        event_tx: tokio::sync::mpsc::UnboundedSender<SessionWorkerEvent>,
    ) {
        spawn_serial_transport_actor(
            session_id,
            self.port,
            self.port_name,
            rx,
            event_tx,
            self.close_ack,
        );
    }
}

#[derive(Clone)]
struct SerialProbeScore {
    port_name: String,
    baud_rate: u32,
    score: f32,
    bytes_read: usize,
    can_open: bool,
    sample: Vec<u8>,
    strong_evidence: bool,
    /// The sample came from the device's own output, without a wake-up CR.
    /// Such a sample is real console text at the right baud, so it can be
    /// accepted without a confirmation round.
    passive: bool,
}

struct SerialDetection {
    confirmed: Option<SerialProbeScore>,
    last_error: Option<ConnectionError>,
}

impl From<SerialProbeScore> for SerialProbeResult {
    fn from(score: SerialProbeScore) -> Self {
        Self {
            port_name: score.port_name,
            baud_rate: score.baud_rate,
            score: score.score,
            bytes_read: score.bytes_read,
            can_open: score.can_open,
        }
    }
}

pub(crate) struct SerialConnectionFactory;

impl SerialConnectionFactory {
    pub(crate) async fn open(
        &self,
        app: AppHandle,
        state: &AppState,
        request: ResolvedConnection,
    ) -> ConnectionResult<ConnectionOpenResult> {
        let requested_port = request
            .serial_port
            .as_deref()
            .or(request.host.as_deref())
            .ok_or_else(|| {
                ConnectionError::new("serial_port_required", "serial port is required", false)
            })?
            .to_string();
        let auto_baud = request.baud_rate.is_none();
        let auto_port = requested_port.eq_ignore_ascii_case("auto");
        let port_candidates = cancelable_open(
            &request,
            resolve_serial_candidates_for_open(state, &requested_port, auto_port, &request.id),
        )
        .await?;
        let serial_settings = SerialLineSettings::from_open_request(&request)?;
        let open_context = request.session_open_context(state);
        let startup_encoding = request.encoding.clone();
        let quick_auto_baud = request.serial_quick_auto_baud.unwrap_or(true);

        let result = cancelable_open(
            &request,
            resolve_serial_open(
                &requested_port,
                port_candidates,
                request.baud_rate,
                if quick_auto_baud {
                    SERIAL_QUICK_AUTO_BAUD_CANDIDATES
                } else {
                    BAUD_CANDIDATES
                },
                if auto_baud && quick_auto_baud {
                    Some(BAUD_CANDIDATES)
                } else {
                    None
                },
                request.encoding.as_deref(),
                serial_settings,
            ),
        )
        .await?;

        let SerialOpenSelection {
            port_name,
            baud_rate,
            scores,
            initial_sample,
            mut port,
        } = result;
        let startup_auth =
            resolve_startup_password_auth(state, &request, "serial_startup_auth_failed")?;
        let initial_sample = if auto_baud {
            initial_sample
        } else if initial_sample.is_empty() {
            cancelable_open(&request, async {
                Ok(read_serial_sample_async(
                    &mut port,
                    Duration::from_millis(SERIAL_FAST_BAUD_SAMPLE_MS),
                )
                .await)
            })
            .await?
        } else {
            initial_sample
        };
        let initial_data =
            decode_serial_startup_sample(&initial_sample, startup_encoding.as_deref());
        let initial_data = (!initial_data.is_empty()).then_some(initial_data);
        log::info!(
            target: "terminal.serial",
            "opening async serial session on {port_name} at {baud_rate} baud auto_port={auto_port} auto_baud={auto_baud}"
        );
        ensure_open_not_cancelled(&request)?;
        ensure_open_current(state, &request)?;
        let (close_ack_tx, close_ack_rx) = tokio::sync::oneshot::channel();
        let session_id = spawn_bound_session(
            app.clone(),
            state,
            BoundSessionOptions {
                session_prefix: "serial",
                connection_id: open_context.connection_id,
                transport: Box::new(SerialSessionTransport {
                    port,
                    port_name: port_name.clone(),
                    close_ack: close_ack_tx,
                }),
                capabilities: crate::terminal::domain::ConnectionCapabilities::serial(),
                codec: open_context.codec,
                initial_data,
                startup_auth,
                resources: TerminalSessionResources::serial(port_name.clone(), close_ack_rx),
                replay_line_limit: open_context.replay_line_limit,
            },
        );

        Ok(ConnectionOpenResult::connected_serial(
            session_id,
            open_context.encoding_label,
            port_name,
            baud_rate,
            scores.into_iter().map(SerialProbeResult::from).collect(),
        ))
    }
}

pub(crate) async fn redetect_serial_baud_on_open_port(
    port_name: &str,
    port: &mut tokio_serial::SerialStream,
    encoding: Option<&str>,
) -> ConnectionResult<SerialRedetectResult> {
    let original_baud_rate = port.baud_rate().map_err(|error| {
        ConnectionError::with_args(
            "serial_baud_read_failed",
            format!("port={port_name}; failed to read current baud rate: {error}"),
            serde_json::json!({ "portName": port_name, "detail": error.to_string() }),
            false,
        )
    })?;
    let mut scores = Vec::new();
    // 两阶段:先扫当前波特率和常用候选;只有快扫见到线路活动时才扩展到完整
    // 列表。全量扫每档要数百毫秒,在静默线路上会拖慢重检测并长时间阻塞会话
    // 关闭;而每档探测都会发 CR 唤醒,接着有设备的线路几乎总会留下字节。
    let quick_candidates = redetect_quick_baud_candidates(original_baud_rate);
    let mut selection =
        match detect_serial_baud(port, port_name, &quick_candidates, encoding, &mut scores).await {
            Ok(detection) => detection.confirmed,
            Err(error) => {
                let _ = set_serial_probe_baud(port, port_name, original_baud_rate);
                prepare_serial_probe_port(port).await;
                return Err(error);
            }
        };
    if selection.is_none() {
        let additional = additional_serial_baud_candidates(&quick_candidates, BAUD_CANDIDATES);
        if should_expand_serial_baud_search(&scores, false, !additional.is_empty()) {
            selection =
                match detect_serial_baud(port, port_name, &additional, encoding, &mut scores).await
                {
                    Ok(detection) => detection.confirmed,
                    Err(error) => {
                        let _ = set_serial_probe_baud(port, port_name, original_baud_rate);
                        prepare_serial_probe_port(port).await;
                        return Err(error);
                    }
                };
        }
    }

    let (baud_rate, confirmed, initial_sample) = if let Some(selection) = selection {
        (selection.baud_rate, true, selection.sample)
    } else {
        set_serial_probe_baud(port, port_name, original_baud_rate)?;
        prepare_serial_probe_port(port).await;
        (original_baud_rate, false, Vec::new())
    };

    Ok(SerialRedetectResult {
        serial_port: port_name.to_string(),
        baud_rate,
        confirmed,
        serial_scores: scores.into_iter().map(SerialProbeResult::from).collect(),
        initial_sample,
    })
}

/// Redetection quick round: the current baud, then the common quick rates.
/// Most redetects confirm within these; the full list is only probed when the
/// quick round saw line activity (see `redetect_serial_baud_on_open_port`).
fn redetect_quick_baud_candidates(current_baud_rate: u32) -> Vec<u32> {
    let mut candidates = vec![current_baud_rate];
    for baud_rate in SERIAL_QUICK_AUTO_BAUD_CANDIDATES {
        if !candidates.contains(baud_rate) {
            candidates.push(*baud_rate);
        }
    }
    candidates
}

async fn resolve_serial_candidates_for_open(
    state: &AppState,
    requested_port: &str,
    auto_port: bool,
    connection_id: &str,
) -> ConnectionResult<Vec<SerialPortCandidate>> {
    let port_candidates = serial_port_candidates(requested_port).await?;
    close_serial_sessions(
        state,
        serial_sessions_for_connection(state, connection_id),
        "reopening the same serial connection",
    )
    .await?;
    if auto_port {
        return available_auto_serial_candidates(state, requested_port, port_candidates);
    }

    close_conflicting_serial_sessions(state, &port_candidates, connection_id).await?;
    Ok(port_candidates)
}

fn available_auto_serial_candidates(
    state: &AppState,
    requested_port: &str,
    port_candidates: Vec<SerialPortCandidate>,
) -> ConnectionResult<Vec<SerialPortCandidate>> {
    let candidate_names: Vec<String> = port_candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    let conflicting_sessions = state.serial_sessions_for_ports(&candidate_names);
    if conflicting_sessions.is_empty() {
        return Ok(port_candidates);
    }

    let occupied_ports: HashSet<String> = conflicting_sessions
        .iter()
        .filter_map(|(_, session)| session.resources.serial_port())
        .map(normalized_serial_port_name)
        .collect();
    let available: Vec<SerialPortCandidate> = port_candidates
        .into_iter()
        .filter(|candidate| !occupied_ports.contains(&normalized_serial_port_name(&candidate.name)))
        .collect();
    if available.is_empty() {
        return Err(ConnectionError::serial_port_unavailable(
            requested_port,
            "all detected serial ports are already active in this workspace".to_string(),
        ));
    }
    log::info!(
        target: "terminal.serial",
        "serial auto port skipped {} active workspace port(s)",
        occupied_ports.len()
    );
    Ok(available)
}

fn normalized_serial_port_name(port: &str) -> String {
    port.trim().to_ascii_lowercase()
}

#[cfg(test)]
fn has_reliable_serial_probe(scores: &[SerialProbeScore]) -> bool {
    scores.iter().any(is_reliable_serial_probe)
}

fn is_reliable_serial_probe(score: &SerialProbeScore) -> bool {
    score.can_open
        && score.strong_evidence
        && score.bytes_read >= 2
        && score.score >= SERIAL_RELIABLE_BAUD_SCORE
}

async fn close_conflicting_serial_sessions(
    state: &AppState,
    port_candidates: &[SerialPortCandidate],
    connection_id: &str,
) -> ConnectionResult<()> {
    let candidate_names: Vec<String> = port_candidates
        .iter()
        .map(|candidate| candidate.name.clone())
        .collect();
    let conflicting_sessions = state.serial_sessions_for_ports(&candidate_names);
    close_serial_sessions(
        state,
        conflicting_sessions,
        &format!("reusing a physical port for connection '{connection_id}'"),
    )
    .await
}

fn serial_sessions_for_connection(
    state: &AppState,
    connection_id: &str,
) -> Vec<(String, TerminalSession)> {
    let session_ids = state.session_ids_for_connection(connection_id);
    let sessions = state.sessions();
    session_ids
        .into_iter()
        .filter_map(|session_id| {
            sessions.get(&session_id).and_then(|session| {
                session
                    .resources
                    .serial_port()
                    .map(|_| (session_id, session.clone()))
            })
        })
        .collect()
}

async fn close_serial_sessions(
    state: &AppState,
    conflicting_sessions: Vec<(String, TerminalSession)>,
    reason: &str,
) -> ConnectionResult<()> {
    if conflicting_sessions.is_empty() {
        return Ok(());
    }

    let port_names: Vec<String> = conflicting_sessions
        .iter()
        .filter_map(|(_, session)| session.resources.serial_port().map(str::to_string))
        .collect();
    // 串口 fd 由传输 actor 持有;worker 退出(会话从状态移除)不代表端口已释放。
    // 取出每个会话的关闭确认,后面等 actor 真正 drop 端口后再放行重开。
    let close_acks: Vec<tokio::sync::oneshot::Receiver<()>> = conflicting_sessions
        .iter()
        .filter_map(|(_, session)| session.resources.take_serial_close_ack())
        .collect();

    for (session_id, session) in &conflicting_sessions {
        log::info!(target: "terminal.serial", "closing serial session '{session_id}' before {reason}");
        let _ = session.tx.send(SessionCommand::Close);
    }

    // 关闭是异步的:worker 需要先排空缓冲输出/重放,再退出并移除会话,随后传输
    // actor 释放串口 fd。给足余量避免在重连时偶发超时;3 秒对正常关闭绰绰有余,
    // 仅在 worker/actor 卡死时触发。
    let deadline = Instant::now() + Duration::from_millis(3_000);
    loop {
        let remaining: Vec<String> = {
            let sessions = state.sessions();
            conflicting_sessions
                .iter()
                .filter(|(session_id, _)| sessions.contains_key(session_id))
                .map(|(session_id, _)| session_id.clone())
                .collect()
        };
        if remaining.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(serial_close_timeout_error(&port_names, reason, &remaining));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // 等 actor 退出:POSIX 下 fd 持有 TIOCEXCL + 独占 flock,不等到这里,
    // 紧接着重开同一端口会得到 EBUSY(表现为"串口被占用")。
    for close_ack in close_acks {
        let remaining = deadline.saturating_duration_since(Instant::now());
        // 发送端随 actor 任务结束必然被 drop,因此收到 Err 同样代表端口已释放;
        // 只有超时(actor 卡死)才算失败。
        if remaining.is_zero() || tokio::time::timeout(remaining, close_ack).await.is_err() {
            log::warn!(
                target: "terminal.serial",
                "serial transport actor still holds the port after close timeout before {reason}"
            );
            return Err(serial_close_timeout_error(&port_names, reason, &[]));
        }
    }
    Ok(())
}

fn serial_close_timeout_error(
    port_names: &[String],
    reason: &str,
    remaining_sessions: &[String],
) -> ConnectionError {
    if !remaining_sessions.is_empty() {
        log::warn!(
            target: "terminal.serial",
            "serial sessions still open after close timeout before {reason}: {remaining_sessions:?}"
        );
    }
    // 用户可见的错误用端口名而非内部会话 ID,并提供可重试的错误码。
    let port_label = if port_names.is_empty() {
        "the serial port".to_string()
    } else {
        port_names.join(", ")
    };
    ConnectionError::with_args(
        "serial_session_close_timeout",
        format!("port={port_label}; timed out waiting for the previous serial session to close"),
        serde_json::json!({ "portName": port_label }),
        true,
    )
}

pub(crate) async fn serial_port_candidates(
    requested_port: &str,
) -> ConnectionResult<Vec<SerialPortCandidate>> {
    if requested_port.eq_ignore_ascii_case("auto") {
        let ports = tokio::task::spawn_blocking(tokio_serial::available_ports)
            .await
            .map_err(|error| {
                ConnectionError::with_args(
                    "serial_port_scan_failed",
                    error.to_string(),
                    serde_json::json!({ "detail": error.to_string() }),
                    true,
                )
            })?
            .map_err(|error| {
                ConnectionError::with_args(
                    "serial_port_scan_failed",
                    error.to_string(),
                    serde_json::json!({ "detail": error.to_string() }),
                    true,
                )
            })?;
        let candidates: Vec<SerialPortCandidate> = ports
            .into_iter()
            .map(|port| SerialPortCandidate {
                usb: matches!(port.port_type, tokio_serial::SerialPortType::UsbPort(_)),
                name: port.port_name,
            })
            .collect();
        if candidates.is_empty() {
            return Err(ConnectionError::serial_port_not_found(
                "auto",
                "no serial ports were found".to_string(),
            ));
        }
        return Ok(candidates);
    }

    Ok(vec![SerialPortCandidate {
        name: requested_port.to_string(),
        usb: false,
    }])
}

async fn resolve_serial_open(
    requested_port: &str,
    port_candidates: Vec<SerialPortCandidate>,
    fixed_baud: Option<u32>,
    auto_baud_candidates: &[u32],
    full_auto_baud_candidates: Option<&[u32]>,
    encoding: Option<&str>,
    settings: SerialLineSettings,
) -> ConnectionResult<SerialOpenSelection> {
    if let Some(baud_rate) = fixed_baud {
        return resolve_fixed_baud(requested_port, port_candidates, baud_rate, settings).await;
    }

    resolve_auto_baud(
        requested_port,
        port_candidates,
        auto_baud_candidates,
        full_auto_baud_candidates,
        encoding,
        settings,
    )
    .await
}

async fn resolve_auto_baud(
    requested_port: &str,
    port_candidates: Vec<SerialPortCandidate>,
    baud_candidates: &[u32],
    full_baud_candidates: Option<&[u32]>,
    encoding: Option<&str>,
    settings: SerialLineSettings,
) -> ConnectionResult<SerialOpenSelection> {
    let mut scores = Vec::new();
    let mut last_error = None;
    let mut fallback_selection: Option<(String, tokio_serial::SerialStream)> = None;

    for candidate in sorted_serial_port_candidates(port_candidates) {
        let port_name = candidate.name;
        let port_score_start = scores.len();
        let initial_baud = baud_candidates
            .first()
            .copied()
            .unwrap_or(SERIAL_FALLBACK_BAUD_RATE);
        let mut port = match open_serial_probe_handle(&port_name, initial_baud, &settings).await {
            Ok(port) => port,
            Err(error) => {
                last_error = Some(error);
                record_failed_serial_probe_scores(&mut scores, &port_name, baud_candidates, false);
                continue;
            }
        };

        let mut detection = detect_serial_baud(
            &mut port,
            &port_name,
            baud_candidates,
            encoding,
            &mut scores,
        )
        .await
        .unwrap_or_else(|error| SerialDetection {
            confirmed: None,
            last_error: Some(error),
        });
        if detection.last_error.is_some() {
            last_error = detection.last_error.take();
        }

        if detection.confirmed.is_none() {
            if let Some(full_candidates) = full_baud_candidates {
                let additional =
                    additional_serial_baud_candidates(baud_candidates, full_candidates);
                if should_expand_serial_baud_search(
                    &scores[port_score_start..],
                    candidate.usb,
                    !additional.is_empty(),
                ) {
                    log::info!(
                        target: "terminal.serial",
                        "serial quick auto baud was not confirmed requested='{requested_port}' port='{port_name}'; expanding candidate search"
                    );
                    let mut expanded = detect_serial_baud(
                        &mut port,
                        &port_name,
                        &additional,
                        encoding,
                        &mut scores,
                    )
                    .await
                    .unwrap_or_else(|error| SerialDetection {
                        confirmed: None,
                        last_error: Some(error),
                    });
                    if expanded.last_error.is_some() {
                        last_error = expanded.last_error.take();
                    }
                    detection.confirmed = expanded.confirmed;
                }
            }
        }

        if let Some(best) = detection.confirmed {
            // Candidates are probed in priority order (USB converters first,
            // then port number), so the first reliably confirmed device is the
            // best answer; scanning the remaining lower-priority ports would
            // only add seconds per dead port.
            log::info!(
                target: "terminal.serial",
                "serial auto baud selected first reliable confirmation requested='{requested_port}' selected='{}@{}' usb={} confidence={:.3} scores=[{}]",
                best.port_name,
                best.baud_rate,
                candidate.usb,
                best.score,
                serial_probe_score_summary(&scores)
            );
            return Ok(SerialOpenSelection {
                port_name: best.port_name,
                baud_rate: best.baud_rate,
                scores,
                initial_sample: best.sample,
                port,
            });
        }

        if fallback_selection.is_none() {
            match set_serial_probe_baud(&mut port, &port_name, SERIAL_FALLBACK_BAUD_RATE) {
                Ok(()) => {
                    prepare_serial_probe_port(&mut port).await;
                    fallback_selection = Some((port_name, port));
                }
                Err(error) => {
                    last_error = Some(error);
                }
            }
        }
    }

    let summary = serial_probe_score_summary(&scores);
    if let Some((port_name, mut port)) = fallback_selection {
        // The probe loop already set this port to the fallback rate; only drain
        // noise that arrived while the remaining ports were being probed.
        prepare_serial_probe_port(&mut port).await;
        log::info!(
            target: "terminal.serial",
            "serial auto baud detection found no reliable candidate requested='{requested_port}'; falling back to '{port_name}@{SERIAL_FALLBACK_BAUD_RATE}' scores=[{summary}]"
        );
        return Ok(SerialOpenSelection {
            port_name,
            baud_rate: SERIAL_FALLBACK_BAUD_RATE,
            scores,
            initial_sample: Vec::new(),
            port,
        });
    }

    log::warn!(
        target: "terminal.serial",
        "serial auto baud detection failed requested='{requested_port}' no openable port scores=[{summary}]"
    );
    Err(last_error.unwrap_or_else(|| {
        ConnectionError::serial_port_not_found(
            requested_port,
            "no serial ports were found or could be opened".to_string(),
        )
    }))
}

/// Probe `baud_candidates` in order on an already open port.
///
/// The first candidate with reliable evidence wins. When the device produced
/// the evidence on its own (passive sample) the baud is clearly right and the
/// sample is real console output, so it is accepted immediately; when the
/// console had to be woken with CR, one confirmation round at the same baud
/// filters out line noise that accidentally decoded as text before the baud
/// is trusted.
async fn detect_serial_baud(
    port: &mut tokio_serial::SerialStream,
    port_name: &str,
    baud_candidates: &[u32],
    encoding: Option<&str>,
    scores: &mut Vec<SerialProbeScore>,
) -> ConnectionResult<SerialDetection> {
    let mut last_error = None;
    for baud_rate in baud_candidates {
        let probe = match probe_open_serial_port(port, port_name, *baud_rate, encoding).await {
            Ok(probe) => probe,
            Err(error) => {
                last_error = Some(error);
                scores.push(failed_serial_probe_score(port_name, *baud_rate, true));
                continue;
            }
        };
        if probe.passive {
            scores.push(probe.clone());
            return Ok(SerialDetection {
                confirmed: Some(probe),
                last_error,
            });
        }
        let reliable = is_reliable_serial_probe(&probe);
        scores.push(probe);
        if !reliable {
            continue;
        }
        let confirmation = confirm_serial_baud(port, port_name, *baud_rate, encoding).await;
        let confirmed = is_reliable_serial_probe(&confirmation);
        scores.push(confirmation.clone());
        if confirmed {
            return Ok(SerialDetection {
                confirmed: Some(confirmation),
                last_error,
            });
        }
    }
    Ok(SerialDetection {
        confirmed: None,
        last_error,
    })
}

/// The full baud sweep costs hundreds of milliseconds per candidate, so it
/// only runs where a live device is plausible: the quick pass saw traffic at
/// a wrong baud, or the port is a USB serial converter — a cable someone
/// plugged in on purpose, unlike an openable but empty onboard port.
fn should_expand_serial_baud_search(
    port_scores: &[SerialProbeScore],
    usb: bool,
    has_additional_candidates: bool,
) -> bool {
    has_additional_candidates && (usb || port_scores.iter().any(|score| score.bytes_read > 0))
}

fn additional_serial_baud_candidates(
    existing_candidates: &[u32],
    full_candidates: &[u32],
) -> Vec<u32> {
    full_candidates
        .iter()
        .copied()
        .filter(|baud_rate| !existing_candidates.contains(baud_rate))
        .collect()
}

async fn resolve_fixed_baud(
    requested_port: &str,
    port_candidates: Vec<SerialPortCandidate>,
    baud_rate: u32,
    settings: SerialLineSettings,
) -> ConnectionResult<SerialOpenSelection> {
    let mut last_error = None;
    for candidate in sorted_serial_port_candidates(port_candidates) {
        match open_serial_selection(candidate.name, baud_rate, &settings).await {
            Ok(selection) => return Ok(selection),
            Err(error) => {
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ConnectionError::serial_port_not_found(
            requested_port,
            "no serial ports were found or could be opened".to_string(),
        )
    }))
}

async fn probe_open_serial_port(
    port: &mut tokio_serial::SerialStream,
    port_name: &str,
    baud_rate: u32,
    encoding: Option<&str>,
) -> ConnectionResult<SerialProbeScore> {
    set_serial_probe_baud(port, port_name, baud_rate)?;
    prepare_serial_probe_port(port).await;
    // Prefer natural console output. Silent devices (common for network equipment)
    // receive one controlled CR for this candidate instead of repeated wakeups.
    let passive_sample =
        read_serial_sample_async(port, Duration::from_millis(SERIAL_PASSIVE_BAUD_SAMPLE_MS)).await;
    let passive_quality = analyze_serial_sample(&passive_sample, encoding);
    let passive = passive_sample.len() >= SERIAL_MIN_DETECT_BYTES
        && passive_quality.confidence >= SERIAL_RELIABLE_BAUD_SCORE
        && passive_quality.strong_evidence;
    let (sample, quality) = if passive {
        (passive_sample, passive_quality)
    } else {
        prepare_serial_probe_port(port).await;
        wake_serial_console(port).await;
        let sample =
            read_serial_sample_async(port, Duration::from_millis(SERIAL_FAST_BAUD_SAMPLE_MS)).await;
        let quality = analyze_serial_sample(&sample, encoding);
        (sample, quality)
    };
    let score = quality.confidence;
    let bytes_read = sample.len();
    log::debug!(
        target: "terminal.serial",
        "serial probe port='{port_name}' baud={baud_rate} score={score:.3} bytes={bytes_read}"
    );
    Ok(SerialProbeScore {
        port_name: port_name.to_string(),
        baud_rate,
        score,
        bytes_read,
        can_open: true,
        sample,
        strong_evidence: quality.strong_evidence,
        passive,
    })
}

fn set_serial_probe_baud(
    port: &mut tokio_serial::SerialStream,
    port_name: &str,
    baud_rate: u32,
) -> ConnectionResult<()> {
    port.set_baud_rate(baud_rate)
        .map_err(|error| classify_serial_config_error(port_name, baud_rate, error))
}

/// Open the port once at the initial candidate baud. Open failures depend on
/// the port/driver, not the baud rate, so retrying other bauds is pointless.
async fn open_serial_probe_handle(
    port_name: &str,
    initial_baud: u32,
    settings: &SerialLineSettings,
) -> ConnectionResult<tokio_serial::SerialStream> {
    open_serial_port_with_busy_retry(port_name, initial_baud, settings)
        .await
        .map_err(|error| classify_serial_open_error(port_name, initial_baud, error))
}

fn failed_serial_probe_score(port_name: &str, baud_rate: u32, can_open: bool) -> SerialProbeScore {
    SerialProbeScore {
        port_name: port_name.to_string(),
        baud_rate,
        score: 0.0,
        bytes_read: 0,
        can_open,
        sample: Vec::new(),
        strong_evidence: false,
        passive: false,
    }
}

fn record_failed_serial_probe_scores(
    scores: &mut Vec<SerialProbeScore>,
    port_name: &str,
    baud_candidates: &[u32],
    can_open: bool,
) {
    scores.extend(
        baud_candidates
            .iter()
            .map(|baud_rate| failed_serial_probe_score(port_name, *baud_rate, can_open)),
    );
}

async fn open_serial_selection(
    port_name: String,
    baud_rate: u32,
    settings: &SerialLineSettings,
) -> ConnectionResult<SerialOpenSelection> {
    let port = open_serial_port_with_busy_retry(&port_name, baud_rate, settings)
        .await
        .map_err(|error| classify_serial_open_error(&port_name, baud_rate, error))?;
    Ok(SerialOpenSelection {
        port_name,
        baud_rate,
        scores: Vec::new(),
        initial_sample: Vec::new(),
        port,
    })
}

/// POSIX 下串口释放不是即时的:fd 持有的 TIOCEXCL/独占 flock 要随 close 解除,
/// USB 串口驱动也有释放延迟;外部程序(如 ModemManager 的 AT 探测)同样会短暂
/// 占用端口。对这些"忙"错误做有限的退避重试,而不是立刻把端口判为不可用。
async fn open_serial_port_with_busy_retry(
    port_name: &str,
    baud_rate: u32,
    settings: &SerialLineSettings,
) -> tokio_serial::Result<tokio_serial::SerialStream> {
    const BUSY_RETRY_BUDGET: Duration = Duration::from_millis(1_200);
    const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(120);
    let started_at = Instant::now();
    loop {
        match open_serial_port(port_name, baud_rate, settings) {
            Err(error)
                if serial_port_error_is_busy(&error)
                    && started_at.elapsed() + BUSY_RETRY_INTERVAL <= BUSY_RETRY_BUDGET =>
            {
                log::info!(
                    target: "terminal.serial",
                    "serial port '{port_name}' is busy; retrying open"
                );
                tokio::time::sleep(BUSY_RETRY_INTERVAL).await;
            }
            result => return result,
        }
    }
}

fn serial_port_error_is_busy(error: &tokio_serial::Error) -> bool {
    let detail = error.to_string().to_lowercase();
    detail.contains("busy") || detail.contains("资源忙")
}

async fn prepare_serial_probe_port(port: &mut tokio_serial::SerialStream) {
    drain_serial_probe_input(port);
    tokio::time::sleep(Duration::from_millis(SERIAL_PROBE_SETTLE_MS)).await;
    drain_serial_probe_input(port);
}

fn drain_serial_probe_input(port: &mut tokio_serial::SerialStream) {
    let mut buffer = [0_u8; SESSION_BUFFER_SIZE];
    let mut drained = 0;
    while drained < SERIAL_SAMPLE_MAX_BYTES {
        let read_limit = buffer.len().min(SERIAL_SAMPLE_MAX_BYTES - drained);
        match port.try_read(&mut buffer[..read_limit]) {
            Ok(0) => return,
            Ok(size) => drained += size,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => return,
            Err(error) => {
                log::debug!(target: "terminal.serial", "serial probe input drain stopped: {error}");
                return;
            }
        }
    }
}

async fn wake_serial_console(port: &mut tokio_serial::SerialStream) {
    if let Err(error) = tokio::io::AsyncWriteExt::write_all(&mut *port, SERIAL_WAKE_SEQUENCE).await
    {
        log::debug!(target: "terminal.serial", "serial wake write failed during baud probe: {error}");
    }
    let _ = tokio::io::AsyncWriteExt::flush(&mut *port).await;
}

async fn stabilize_serial_console(port: &mut tokio_serial::SerialStream) -> Vec<u8> {
    // A wrong-baud probe can leave a byte in the device's own line editor; host
    // buffer clearing cannot remove it. At the selected baud, terminate that
    // possible residual line and discard its response, then request one clean
    // prompt. Only the clean response is handed to the terminal.
    prepare_serial_probe_port(port).await;
    wake_serial_console(port).await;
    let _ = read_serial_sample_async(port, Duration::from_millis(SERIAL_PROBE_MAX_SAMPLE_MS)).await;
    prepare_serial_probe_port(port).await;
    wake_serial_console(port).await;
    read_serial_sample_async(port, Duration::from_millis(SERIAL_PROBE_MAX_SAMPLE_MS)).await
}

/// Confirmation runs at the same baud as the probe round that just succeeded,
/// so the port is already configured; only the console needs stabilizing.
async fn confirm_serial_baud(
    port: &mut tokio_serial::SerialStream,
    port_name: &str,
    baud_rate: u32,
    encoding: Option<&str>,
) -> SerialProbeScore {
    let sample = stabilize_serial_console(port).await;
    let quality = analyze_serial_sample(&sample, encoding);
    SerialProbeScore {
        port_name: port_name.to_string(),
        baud_rate,
        score: quality.confidence,
        bytes_read: sample.len(),
        can_open: true,
        sample,
        strong_evidence: quality.strong_evidence,
        passive: false,
    }
}

async fn read_serial_sample_async(
    port: &mut tokio_serial::SerialStream,
    sample_for: Duration,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(SERIAL_SAMPLE_MAX_BYTES);
    let mut buffer = vec![0_u8; SERIAL_SAMPLE_MAX_BYTES.min(SESSION_BUFFER_SIZE)];
    read_serial_sample_into(port, sample_for, &mut output, &mut buffer).await;
    output
}

async fn read_serial_sample_into(
    port: &mut tokio_serial::SerialStream,
    sample_for: Duration,
    output: &mut Vec<u8>,
    buffer: &mut [u8],
) {
    let started_at = Instant::now();
    let initial_deadline = started_at + sample_for;
    let hard_deadline =
        started_at + Duration::from_millis(SERIAL_PROBE_MAX_SAMPLE_MS).max(sample_for);
    let mut quiet_deadline = None;
    while output.len() < SERIAL_SAMPLE_MAX_BYTES {
        let now = Instant::now();
        let active_deadline = quiet_deadline
            .unwrap_or(initial_deadline)
            .min(hard_deadline);
        if now >= active_deadline {
            break;
        }
        let remaining = active_deadline.saturating_duration_since(now);
        let read_limit = buffer.len().min(SERIAL_SAMPLE_MAX_BYTES - output.len());
        match port.try_read(&mut buffer[..read_limit]) {
            Ok(size) if size > 0 => {
                output.extend_from_slice(&buffer[..size]);
                quiet_deadline = Some(
                    (Instant::now() + Duration::from_millis(SERIAL_PROBE_INTER_BYTE_TIMEOUT_MS))
                        .min(hard_deadline),
                );
                continue;
            }
            Err(error) if error.kind() != ErrorKind::WouldBlock => {
                log::debug!(target: "terminal.serial", "serial sample read failed: {error}");
                break;
            }
            // try_read reports an empty poll as Ok(0) or WouldBlock depending on
            // the driver; both mean "no bytes right now", so back off briefly.
            // Do not wrap `SerialStream::readable` in a timeout here: dropping
            // that readiness future cancels the overlapped read on Windows and
            // can leave ERROR_OPERATION_ABORTED (995) for the session actor that
            // takes ownership of this same handle after auto detection.
            _ => {}
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(10))).await;
    }
}

fn serial_probe_score_summary(scores: &[SerialProbeScore]) -> String {
    if scores.is_empty() {
        return "none".to_string();
    }

    scores
        .iter()
        .map(|score| {
            format!(
                "{}@{} score={:.3} bytes={} open={}",
                score.port_name, score.baud_rate, score.score, score.bytes_read, score.can_open
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn sorted_serial_port_candidates(mut ports: Vec<SerialPortCandidate>) -> Vec<SerialPortCandidate> {
    ports.sort_by(|a, b| {
        b.usb
            .cmp(&a.usb)
            .then_with(|| compare_serial_port_names(&a.name, &b.name))
    });
    ports
}

pub(super) fn classify_serial_open_error(
    port_name: &str,
    baud_rate: u32,
    error: tokio_serial::Error,
) -> ConnectionError {
    log::warn!(target: "terminal.serial", "failed to open serial port '{port_name}' at {baud_rate} baud: {error}");
    let detail = error.to_string();
    // Linux 串口设备节点默认归属 root:dialout（Arch 等为 uucp），EACCES 几乎
    // 总是当前用户不在设备组；返回独立错误码，让前端给出“加入用户组”的可操作
    // 提示，而不是笼统的“端口不可用”。Windows 的 ACCESS_DENIED 多为端口被
    // 占用，仍按不可用处理。
    #[cfg(target_os = "linux")]
    if matches!(
        error.kind(),
        tokio_serial::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)
    ) {
        return ConnectionError::with_args(
            "serial_port_permission_denied",
            format!("port={port_name}; {detail}"),
            serde_json::json!({ "portName": port_name, "detail": detail }),
            true,
        );
    }
    if serial_error_is_unavailable(&error) {
        return ConnectionError::serial_port_unavailable(port_name, detail);
    }
    ConnectionError::with_args(
        "serial_port_open_failed",
        format!("port={port_name}; baud={baud_rate}; {detail}"),
        serde_json::json!({ "portName": port_name, "baudRate": baud_rate, "detail": detail }),
        true,
    )
}

fn decode_serial_startup_sample(sample: &[u8], encoding: Option<&str>) -> String {
    if sample.is_empty() {
        return String::new();
    }
    let Some(label) = encoding.and_then(|value| Encoding::for_label(value.as_bytes())) else {
        return String::from_utf8_lossy(sample).into_owned();
    };
    let (decoded, _, _) = label.decode(sample);
    decoded.into_owned()
}

fn classify_serial_config_error(
    port_name: &str,
    baud_rate: u32,
    error: tokio_serial::Error,
) -> ConnectionError {
    log::warn!(target: "terminal.serial", "failed to configure serial port '{port_name}' at {baud_rate} baud: {error}");
    let detail = error.to_string();
    if serial_error_is_unavailable(&error) {
        return ConnectionError::serial_port_unavailable(port_name, detail);
    }
    ConnectionError::with_args(
        "serial_port_config_failed",
        format!("port={port_name}; baud={baud_rate}; {detail}"),
        serde_json::json!({ "portName": port_name, "baudRate": baud_rate, "detail": detail }),
        true,
    )
}

const SERIAL_UNAVAILABLE_ERROR_MARKERS: &[&str] = &[
    "access denied",
    "permission denied",
    "拒绝访问",
    "device is not functioning",
    "attached to the system is not functioning",
    "device is not ready",
    "device not ready",
    "device unavailable",
    "设备不可用",
    "设备未就绪",
    "设备没有发挥作用",
    "设备不能正常运行",
    "系统找不到指定的文件",
    "invalid handle",
    "句柄无效",
    "incorrect function",
    "函数不正确",
    "being used",
    "in use",
    // Linux EBUSY("Device or resource busy"):端口被 TIOCEXCL/flock 占用,
    // 常见于上一会话尚未释放或 ModemManager 等外部程序探测期间。
    "resource busy",
    "设备或资源忙",
    "占用",
];

fn serial_error_is_unavailable(error: &tokio_serial::Error) -> bool {
    matches!(
        error.kind(),
        tokio_serial::ErrorKind::NoDevice
            | tokio_serial::ErrorKind::Io(std::io::ErrorKind::NotFound)
            | tokio_serial::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)
    ) || serial_error_looks_unavailable(&error.to_string())
}

pub(super) fn serial_error_looks_unavailable(detail: &str) -> bool {
    let normalized = detail.to_lowercase();
    SERIAL_UNAVAILABLE_ERROR_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{
        additional_serial_baud_candidates, has_reliable_serial_probe, is_reliable_serial_probe,
        redetect_quick_baud_candidates, serial_error_is_unavailable,
        serial_error_looks_unavailable, serial_port_error_is_busy,
        should_expand_serial_baud_search, sorted_serial_port_candidates, SerialPortCandidate,
        SerialProbeScore,
    };
    use crate::terminal::internal::core::BAUD_CANDIDATES;
    use std::collections::HashSet;

    fn score(baud_rate: u32, value: f32, bytes_read: usize) -> SerialProbeScore {
        SerialProbeScore {
            port_name: "COM1".to_string(),
            baud_rate,
            score: value,
            bytes_read,
            can_open: true,
            sample: vec![b'x'; bytes_read],
            strong_evidence: bytes_read >= 4,
            passive: false,
        }
    }

    #[test]
    fn low_confidence_noise_is_not_a_baud_candidate() {
        let scores = [score(9_600, 0.45, 4), score(115_200, 0.20, 32)];
        assert!(!has_reliable_serial_probe(&scores));
    }

    #[test]
    fn reliable_baud_requires_enough_data_and_strong_evidence() {
        assert!(has_reliable_serial_probe(&[score(115_200, 0.82, 12)]));
        assert!(!is_reliable_serial_probe(&score(9_600, 0.90, 3)));
        assert!(!is_reliable_serial_probe(&score(38_400, 0.10, 64)));
    }

    #[test]
    fn short_sample_requires_explicit_strong_evidence() {
        let mut short_prompt = score(115_200, 0.90, 2);
        assert!(!is_reliable_serial_probe(&short_prompt));

        short_prompt.strong_evidence = true;
        assert!(is_reliable_serial_probe(&short_prompt));
    }

    #[test]
    fn redetection_quick_round_prioritizes_current_and_common_rates() {
        assert_eq!(
            redetect_quick_baud_candidates(38_400),
            vec![38_400, 9_600, 115_200]
        );
        assert_eq!(redetect_quick_baud_candidates(9_600), vec![9_600, 115_200]);
        assert_eq!(
            redetect_quick_baud_candidates(115_200),
            vec![115_200, 9_600]
        );
    }

    #[test]
    fn redetection_expansion_covers_all_remaining_known_rates() {
        let quick = redetect_quick_baud_candidates(38_400);
        let additional = additional_serial_baud_candidates(&quick, BAUD_CANDIDATES);
        let all: HashSet<u32> = quick.iter().chain(additional.iter()).copied().collect();
        assert!(BAUD_CANDIDATES
            .iter()
            .all(|baud_rate| all.contains(baud_rate)));
        assert_eq!(all.len(), quick.len() + additional.len());
    }

    #[test]
    fn expanded_search_contains_only_candidates_not_already_probed() {
        let additional =
            additional_serial_baud_candidates(&[9_600, 115_200], &[9_600, 38_400, 115_200]);

        assert_eq!(additional, vec![38_400]);
    }

    #[test]
    fn full_baud_sweep_requires_traffic_or_a_usb_port() {
        let silent = [score(9_600, 0.0, 0), score(115_200, 0.0, 0)];
        let noisy = [score(9_600, 0.10, 6), score(115_200, 0.0, 0)];

        assert!(!should_expand_serial_baud_search(&silent, false, true));
        assert!(should_expand_serial_baud_search(&noisy, false, true));
        assert!(should_expand_serial_baud_search(&silent, true, true));
        assert!(!should_expand_serial_baud_search(&noisy, true, false));
    }

    #[test]
    fn exhaustive_search_checks_common_rates_before_legacy_and_high_speed_rates() {
        assert_eq!(&BAUD_CANDIDATES[..3], &[9_600, 115_200, 38_400]);
        let legacy_position = BAUD_CANDIDATES
            .iter()
            .position(|rate| *rate == 300)
            .unwrap();
        let maximum_position = BAUD_CANDIDATES
            .iter()
            .position(|rate| *rate == 2_000_000)
            .unwrap();
        assert!(legacy_position < maximum_position);
    }

    #[test]
    fn usb_serial_ports_are_probed_before_native_ports() {
        let candidate = |name: &str, usb: bool| SerialPortCandidate {
            name: name.to_string(),
            usb,
        };
        let sorted = sorted_serial_port_candidates(vec![
            candidate("COM1", false),
            candidate("COM7", true),
            candidate("COM3", false),
            candidate("COM5", true),
        ]);
        let names: Vec<&str> = sorted.iter().map(|entry| entry.name.as_str()).collect();

        assert_eq!(names, ["COM5", "COM7", "COM1", "COM3"]);
    }

    #[test]
    fn serial_unavailable_detection_prefers_error_kind() {
        assert!(serial_error_is_unavailable(&tokio_serial::Error::new(
            tokio_serial::ErrorKind::NoDevice,
            "driver-specific text"
        )));
        assert!(serial_error_is_unavailable(&tokio_serial::Error::new(
            tokio_serial::ErrorKind::Io(std::io::ErrorKind::PermissionDenied),
            "driver-specific text"
        )));
    }

    #[test]
    fn serial_unavailable_detection_covers_windows_device_errors() {
        assert!(serial_error_looks_unavailable(
            "A device attached to the system is not functioning."
        ));
        assert!(serial_error_looks_unavailable("The device is not ready."));
        assert!(serial_error_looks_unavailable("设备不可用。"));
        assert!(serial_error_looks_unavailable("函数不正确。"));
    }

    #[test]
    fn serial_unavailable_detection_covers_contention_errors() {
        assert!(serial_error_looks_unavailable("Access denied."));
        assert!(serial_error_looks_unavailable(
            "The requested resource is in use."
        ));
        assert!(serial_error_looks_unavailable("端口已被占用"));
    }

    #[test]
    fn serial_unavailable_detection_covers_linux_busy_errors() {
        // EBUSY:上一会话的 fd 尚未释放(TIOCEXCL/flock)或外部程序短暂占用。
        assert!(serial_error_looks_unavailable(
            "EBUSY: Device or resource busy (os error 16)"
        ));
        assert!(serial_error_looks_unavailable("设备或资源忙"));
    }

    #[test]
    fn serial_busy_retry_matches_only_busy_errors() {
        let busy = tokio_serial::Error::new(
            tokio_serial::ErrorKind::NoDevice,
            "Device or resource busy (os error 16)",
        );
        let missing = tokio_serial::Error::new(
            tokio_serial::ErrorKind::NoDevice,
            "No such file or directory",
        );

        assert!(serial_port_error_is_busy(&busy));
        assert!(!serial_port_error_is_busy(&missing));
    }
}
