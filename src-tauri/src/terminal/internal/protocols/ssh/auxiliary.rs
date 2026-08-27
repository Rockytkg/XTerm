use super::ssh_runtime_metrics_script::{runtime_metrics_command, runtime_metrics_script_version};
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

pub(crate) async fn ssh_exec_capture(
    session: &SharedSshSession,
    command: &str,
) -> Result<String, String> {
    let channel = {
        let session = session.lock().await;
        session
            .channel_open_session()
            .await
            .map_err(|error| format!("failed to open SSH exec channel: {error}"))?
    };
    channel
        .exec(true, command)
        .await
        .map_err(|error| format!("failed to execute remote command: {error}"))?;
    let mut output = Vec::new();
    let mut channel = channel;
    while let Some(message) = channel.wait().await {
        match message {
            russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. } => {
                output.extend_from_slice(&data);
            }
            russh::ChannelMsg::Eof | russh::ChannelMsg::Close => break,
            _ => {}
        }
    }
    String::from_utf8(output).map_err(|error| format!("remote output was not UTF-8: {error}"))
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
    let monitor_interval = Duration::from_secs(2);
    let detail_sample_every = 5;
    // 单次采样失败（通道争用、远端瞬时高负载）不应终止监控；
    // 连续失败达到上限才判定采样不可用并退出，由前端展示不可用状态。
    const MAX_CONSECUTIVE_FAILURES: u32 = 3;
    let mut consecutive_failures = 0_u32;
    let mut sample_index = 0_u64;
    let mut previous_cpu_total = None;
    let mut previous_cpu_idle = None;
    let mut previous_cpu_user = None;
    let mut previous_cpu_system = None;
    let mut previous_cpu_iowait = None;
    let mut previous_cpu_steal = None;
    let mut previous_network_rx_bytes = None;
    let mut previous_network_tx_bytes = None;
    let mut previous_sample_at: Option<Instant> = None;

    loop {
        if !state.monitor_task_matches(&session_id, &guard) {
            break;
        }
        let include_detail = sample_index.is_multiple_of(detail_sample_every);
        let command = runtime_metrics_command(include_detail);
        log::trace!(
            target: "ssh.auxiliary",
            "runtime metrics sample command prepared for '{session_id}' include_detail={include_detail} script_version={} command_chars={}",
            runtime_metrics_script_version(),
            command.len()
        );
        let result = {
            ssh_exec_capture(&session, &command)
                .await
                .and_then(|output| parse_runtime_metrics(&output))
        };
        if !state.monitor_task_matches(&session_id, &guard) {
            break;
        }
        match result {
            Ok(mut metrics) => {
                consecutive_failures = 0;
                let completed_at = Instant::now();
                sample_index = sample_index.saturating_add(1);
                if let (Some(total), Some(idle), Some(pt), Some(pi)) = (
                    metrics.cpu_total,
                    metrics.cpu_idle,
                    previous_cpu_total,
                    previous_cpu_idle,
                ) {
                    let td = total.saturating_sub(pt);
                    let id = idle.saturating_sub(pi);
                    if td > 0 {
                        metrics.cpu_percent = (td.saturating_sub(id) as f32 * 100.0) / td as f32;
                        metrics.cpu_user_percent =
                            cpu_delta_percent(metrics.cpu_user, previous_cpu_user, td);
                        metrics.cpu_system_percent =
                            cpu_delta_percent(metrics.cpu_system, previous_cpu_system, td);
                        metrics.cpu_iowait_percent =
                            cpu_delta_percent(metrics.cpu_iowait, previous_cpu_iowait, td);
                        metrics.cpu_steal_percent =
                            cpu_delta_percent(metrics.cpu_steal, previous_cpu_steal, td);
                        metrics.cpu_ready = true;
                    }
                }
                if let (
                    Some(rx),
                    Some(tx),
                    Some(previous_rx),
                    Some(previous_tx),
                    Some(previous_at),
                ) = (
                    metrics.network_rx_bytes,
                    metrics.network_tx_bytes,
                    previous_network_rx_bytes,
                    previous_network_tx_bytes,
                    previous_sample_at,
                ) {
                    let elapsed = completed_at.duration_since(previous_at).as_secs_f32();
                    if elapsed > 0.0 {
                        metrics.network_rx_rate =
                            Some(rx.saturating_sub(previous_rx) as f32 / elapsed);
                        metrics.network_tx_rate =
                            Some(tx.saturating_sub(previous_tx) as f32 / elapsed);
                    }
                }
                previous_cpu_total = metrics.cpu_total;
                previous_cpu_idle = metrics.cpu_idle;
                previous_cpu_user = metrics.cpu_user;
                previous_cpu_system = metrics.cpu_system;
                previous_cpu_iowait = metrics.cpu_iowait;
                previous_cpu_steal = metrics.cpu_steal;
                previous_network_rx_bytes = metrics.network_rx_bytes;
                previous_network_tx_bytes = metrics.network_tx_bytes;
                previous_sample_at = Some(completed_at);
                metrics.connection_id = connection_id.clone();
                metrics.session_id = session_id.clone();
                metrics.latency_ms = measure_ssh_protocol_latency(&session).await;
                if !state.monitor_task_matches(&session_id, &guard) {
                    break;
                }
                metrics.sample_timestamp_ms = unix_timestamp_ms();
                emit_ssh_runtime_metrics(&app, metrics);
            }
            Err(error) => {
                consecutive_failures += 1;
                log::warn!(
                    target: "ssh.auxiliary",
                    "runtime metrics sample failed for '{session_id}' ({consecutive_failures}/{MAX_CONSECUTIVE_FAILURES}): {error}"
                );
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    let mut metrics = empty_runtime_metrics();
                    metrics.connection_id = connection_id.clone();
                    metrics.session_id = session_id.clone();
                    metrics.sample_timestamp_ms = unix_timestamp_ms();
                    metrics.unavailable = true;
                    emit_ssh_runtime_metrics(&app, metrics);
                    state.remove_monitor_task(&session_id);
                    return Err(error);
                }
                // 未达上限：保留上一份数据，等待下个周期重试。
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(monitor_interval) => {}
            _ = wait_until_monitor_stopped(state, &session_id, &guard) => break,
        }
    }
    if state.monitor_task_matches(&session_id, &guard) {
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
