use super::ssh_runtime_metrics_script::{
    runtime_metrics_script_version, runtime_metrics_stream_command, SAMPLE_BLOCK_DELIMITER,
    SAMPLE_INTERVAL_SECONDS,
};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use russh_sftp::client::SftpSession as RusshSftpSession;
use tauri::{AppHandle, Manager};

use crate::{
    state::{AppState, SftpSession},
    terminal::{
        events::emit_ssh_runtime_metrics,
        internal::{
            core::SshRuntimeMetrics,
            ssh_client::SharedSshSession,
            ssh_metrics::{empty_runtime_metrics, parse_runtime_metrics},
        },
    },
};

pub(super) fn validate_aux_ssh_target(
    state: &AppState,
    connection_id: &str,
    session_id: &str,
) -> Result<(), String> {
    let bound = state.connection_id_for_session(session_id).ok_or_else(|| {
        "An active terminal session is required for SSH auxiliary work.".to_string()
    })?;
    if bound != connection_id {
        return Err(
            "The active terminal session does not match the requested connection.".to_string(),
        );
    }
    let session = state.sessions().get(session_id).cloned().ok_or_else(|| {
        "An active terminal session is required for SSH auxiliary work.".to_string()
    })?;
    if session.capabilities.sftp && session.resources.ssh_aux_session().is_some() {
        Ok(())
    } else {
        Err("SFTP and runtime metrics require an SSH-capable session".to_string())
    }
}

fn shared_ssh_session(
    state: &AppState,
    connection_id: &str,
    session_id: &str,
) -> Result<SharedSshSession, String> {
    validate_aux_ssh_target(state, connection_id, session_id)?;
    let session = state.sessions().get(session_id).cloned().ok_or_else(|| {
        "An active terminal session is required for SSH auxiliary work.".to_string()
    })?;
    session
        .resources
        .ssh_aux_session()
        .ok_or_else(|| "The active backend session is not an SSH session.".to_string())
}

pub(crate) async fn get_or_create_sftp_session(
    state: &AppState,
    connection_id: &str,
    session_id: &str,
) -> Result<SftpSession, String> {
    validate_aux_ssh_target(state, connection_id, session_id)?;
    if let Some(existing) = state.sftp_session(session_id) {
        if !existing.is_closed() {
            log::debug!(target: "ssh.auxiliary", "reusing sftp session for '{session_id}'");
            return Ok(existing);
        }
        state.invalidate_sftp_session(session_id);
    }
    let session_lock = state.sftp_session_lock(session_id);
    let _session_guard = session_lock.lock().await;
    if let Some(existing) = state.sftp_session(session_id) {
        if !existing.is_closed() {
            log::debug!(target: "ssh.auxiliary", "reusing sftp session for '{session_id}' after init wait");
            return Ok(existing);
        }
        state.invalidate_sftp_session(session_id);
    }
    log::info!(target: "ssh.auxiliary", "opening async sftp session for '{session_id}' connection='{connection_id}'");
    let session = shared_ssh_session(state, connection_id, session_id)?;
    let channel = {
        let session = session.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|error| format!("failed to open SFTP channel: {error}"))?
    };
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| format!("failed to request SFTP subsystem: {error}"))?;
    let sftp = RusshSftpSession::new(channel.into_stream())
        .await
        .map_err(|error| format!("failed to initialize SFTP subsystem: {error}"))?;
    validate_aux_ssh_target(state, connection_id, session_id)?;
    let sftp_session = SftpSession::new(sftp);
    state.bind_sftp_session(session_id, sftp_session.clone());
    log::info!(target: "ssh.auxiliary", "sftp session ready for '{session_id}'");
    Ok(sftp_session)
}

async fn measure_ssh_protocol_latency(session: &SharedSshSession) -> Option<f32> {
    const LATENCY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

    let started_at = Instant::now();
    let result = {
        let session = session.lock().await;
        tokio::time::timeout(LATENCY_PROBE_TIMEOUT, session.send_ping()).await
    };
    match result {
        Ok(Ok(())) => Some((started_at.elapsed().as_secs_f64() * 1000.0) as f32),
        Ok(Err(error)) => {
            log::debug!(target: "ssh.auxiliary", "ssh protocol latency probe failed: {error}");
            None
        }
        Err(_) => {
            log::debug!(
                target: "ssh.auxiliary",
                "ssh protocol latency probe timed out after {}ms",
                LATENCY_PROBE_TIMEOUT.as_millis()
            );
            None
        }
    }
}

/// 跨采样保留的计数器快照：CPU 占用率与网络/磁盘速率都依赖相邻两次采样的差分。
#[derive(Default)]
struct MetricsDiffState {
    cpu_total: Option<u64>,
    cpu_idle: Option<u64>,
    cpu_user: Option<u64>,
    cpu_system: Option<u64>,
    cpu_iowait: Option<u64>,
    cpu_steal: Option<u64>,
    network_rx_bytes: Option<u64>,
    network_tx_bytes: Option<u64>,
    disk_read_bytes: Option<u64>,
    disk_write_bytes: Option<u64>,
    sample_at: Option<Instant>,
}

impl MetricsDiffState {
    fn complete(&mut self, metrics: &mut SshRuntimeMetrics, completed_at: Instant) {
        if let (Some(total), Some(idle), Some(previous_total), Some(previous_idle)) = (
            metrics.cpu_total,
            metrics.cpu_idle,
            self.cpu_total,
            self.cpu_idle,
        ) {
            let total_delta = total.saturating_sub(previous_total);
            let idle_delta = idle.saturating_sub(previous_idle);
            if total_delta > 0 {
                metrics.cpu_percent =
                    (total_delta.saturating_sub(idle_delta) as f32 * 100.0) / total_delta as f32;
                metrics.cpu_user_percent =
                    cpu_delta_percent(metrics.cpu_user, self.cpu_user, total_delta);
                metrics.cpu_system_percent =
                    cpu_delta_percent(metrics.cpu_system, self.cpu_system, total_delta);
                metrics.cpu_iowait_percent =
                    cpu_delta_percent(metrics.cpu_iowait, self.cpu_iowait, total_delta);
                metrics.cpu_steal_percent =
                    cpu_delta_percent(metrics.cpu_steal, self.cpu_steal, total_delta);
                metrics.cpu_ready = true;
            }
        }
        let elapsed = self
            .sample_at
            .map(|previous_at| completed_at.duration_since(previous_at).as_secs_f32())
            .filter(|value| *value > 0.0);
        if let (Some(elapsed), Some(rx), Some(tx), Some(previous_rx), Some(previous_tx)) = (
            elapsed,
            metrics.network_rx_bytes,
            metrics.network_tx_bytes,
            self.network_rx_bytes,
            self.network_tx_bytes,
        ) {
            metrics.network_rx_rate = Some(rx.saturating_sub(previous_rx) as f32 / elapsed);
            metrics.network_tx_rate = Some(tx.saturating_sub(previous_tx) as f32 / elapsed);
        }
        if let (Some(elapsed), Some(read), Some(write), Some(previous_read), Some(previous_write)) = (
            elapsed,
            metrics.disk_read_bytes,
            metrics.disk_write_bytes,
            self.disk_read_bytes,
            self.disk_write_bytes,
        ) {
            metrics.disk_read_rate = Some(read.saturating_sub(previous_read) as f32 / elapsed);
            metrics.disk_write_rate = Some(write.saturating_sub(previous_write) as f32 / elapsed);
        }
        self.cpu_total = metrics.cpu_total;
        self.cpu_idle = metrics.cpu_idle;
        self.cpu_user = metrics.cpu_user;
        self.cpu_system = metrics.cpu_system;
        self.cpu_iowait = metrics.cpu_iowait;
        self.cpu_steal = metrics.cpu_steal;
        self.network_rx_bytes = metrics.network_rx_bytes;
        self.network_tx_bytes = metrics.network_tx_bytes;
        self.disk_read_bytes = metrics.disk_read_bytes;
        self.disk_write_bytes = metrics.disk_write_bytes;
        self.sample_at = Some(completed_at);
    }
}

struct MetricsMonitorContext<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
    session: &'a SharedSshSession,
    connection_id: &'a str,
    session_id: &'a str,
    guard: &'a Arc<()>,
}

impl MetricsMonitorContext<'_> {
    fn is_active(&self) -> bool {
        self.state.monitor_task_matches(self.session_id, self.guard)
    }

    fn emit_unavailable(&self) {
        let mut metrics = empty_runtime_metrics();
        metrics.connection_id = self.connection_id.to_string();
        metrics.session_id = self.session_id.to_string();
        metrics.sample_timestamp_ms = unix_timestamp_ms();
        metrics.unavailable = true;
        emit_ssh_runtime_metrics(self.app, metrics);
    }
}

/// 从流缓冲中取出下一个完整采样块（分隔符之前的内容）。
/// 块不完整时返回 None，残余数据留在缓冲里等待后续分片。
fn take_sample_block(buffer: &mut String) -> Option<String> {
    let end = buffer.find(SAMPLE_BLOCK_DELIMITER)?;
    let mut block: String = buffer.drain(..end + SAMPLE_BLOCK_DELIMITER.len()).collect();
    block.truncate(end);
    if buffer.starts_with('\n') {
        buffer.drain(..1);
    }
    Some(block)
}

async fn open_metrics_stream(
    session: &SharedSshSession,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    let channel = {
        let session = session.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|error| format!("failed to open runtime metrics channel: {error}"))?
    };
    channel
        .exec(true, runtime_metrics_stream_command())
        .await
        .map_err(|error| format!("failed to start runtime metrics stream: {error}"))?;
    Ok(channel)
}

async fn process_sample(ctx: &MetricsMonitorContext<'_>, diff: &mut MetricsDiffState, block: &str) {
    let mut metrics = match parse_runtime_metrics(block) {
        Ok(metrics) => metrics,
        Err(error) => {
            // 单个块损坏（通道分片截断已由分隔符协议兜底，这里防御远端输出异常）：
            // 跳过该块即可，流本身仍然健康。
            log::warn!(
                target: "ssh.auxiliary",
                "runtime metrics sample parse failed for '{}': {error}",
                ctx.session_id
            );
            return;
        }
    };
    diff.complete(&mut metrics, Instant::now());
    metrics.connection_id = ctx.connection_id.to_string();
    metrics.session_id = ctx.session_id.to_string();
    metrics.latency_ms = measure_ssh_protocol_latency(ctx.session).await;
    // 延迟探测最长 3s，期间用户可能已关闭监控，落点处再校验一次避免发出过期样本。
    if !ctx.is_active() {
        return;
    }
    metrics.sample_timestamp_ms = unix_timestamp_ms();
    emit_ssh_runtime_metrics(ctx.app, metrics);
}

/// 单 exec 通道流式监控：远端脚本自循环推送采样块，本函数按分隔符增量解析并下发。
/// 远端只运行一个采样进程，相比逐次开通道的旧实现，采样间隔精确、远端开销恒定。
pub(crate) async fn run_runtime_metrics_monitor(
    app: AppHandle,
    connection_id: String,
    session_id: String,
    guard: Arc<()>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let state = state.inner();
    log::info!(
        target: "ssh.auxiliary",
        "runtime metrics monitor starting for '{session_id}' script_version={}",
        runtime_metrics_script_version()
    );
    let session = shared_ssh_session(state, &connection_id, &session_id)?;
    let ctx = MetricsMonitorContext {
        app: &app,
        state,
        session: &session,
        connection_id: &connection_id,
        session_id: &session_id,
        guard: &guard,
    };

    // 建流失败（通道受限、远端拒绝 exec）重试数次后上报不可用，与旧实现的用户可见语义一致。
    const MAX_STREAM_ATTEMPTS: u32 = 3;
    let mut attempt = 0_u32;
    let mut channel = loop {
        attempt += 1;
        match open_metrics_stream(&session).await {
            Ok(channel) => break channel,
            Err(error) => {
                log::warn!(
                    target: "ssh.auxiliary",
                    "runtime metrics stream setup failed for '{session_id}' ({attempt}/{MAX_STREAM_ATTEMPTS}): {error}"
                );
                if attempt >= MAX_STREAM_ATTEMPTS {
                    ctx.emit_unavailable();
                    state.remove_monitor_task(&session_id);
                    return Err(error);
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(SAMPLE_INTERVAL_SECONDS)) => {}
                    _ = wait_until_monitor_stopped(state, &session_id, &guard) => {
                        log::info!(target: "ssh.auxiliary", "runtime metrics monitor stopped for '{session_id}'");
                        return Ok(());
                    }
                }
            }
        }
    };

    let mut diff = MetricsDiffState::default();
    let mut buffer = String::new();
    let mut stream_error: Option<String> = None;
    loop {
        if !ctx.is_active() {
            break;
        }
        let message = tokio::select! {
            message = channel.wait() => message,
            _ = wait_until_monitor_stopped(state, &session_id, &guard) => None,
        };
        let Some(message) = message else {
            // wait() 返回 None 表示流被对端关闭；若非本地主动停止则判定采样不可用。
            if ctx.is_active() {
                stream_error = Some("runtime metrics stream closed by remote".to_string());
            }
            break;
        };
        match message {
            russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                buffer.push_str(&String::from_utf8_lossy(&data));
                while ctx.is_active() {
                    let Some(block) = take_sample_block(&mut buffer) else {
                        break;
                    };
                    process_sample(&ctx, &mut diff, &block).await;
                }
            }
            russh::ChannelMsg::Eof | russh::ChannelMsg::Close => {}
            _ => {}
        }
    }
    // 主动关闭通道，让远端采样循环立即退出，而不是等下一次写时才收到 SIGPIPE。
    let _ = channel.close().await;
    if let Some(error) = stream_error {
        ctx.emit_unavailable();
        state.remove_monitor_task(&session_id);
        return Err(error);
    }
    if ctx.is_active() {
        state.remove_monitor_task(&session_id);
    }
    log::info!(target: "ssh.auxiliary", "runtime metrics monitor stopped for '{session_id}'");
    Ok(())
}

async fn wait_until_monitor_stopped(state: &AppState, session_id: &str, guard: &Arc<()>) {
    while state.monitor_task_matches(session_id, guard) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn cpu_delta_percent(current: Option<u64>, previous: Option<u64>, total_delta: u64) -> Option<f32> {
    if total_delta == 0 {
        return None;
    }
    Some((current?.saturating_sub(previous?) as f32 * 100.0) / total_delta as f32)
        .map(|value| value.clamp(0.0, 100.0))
}
