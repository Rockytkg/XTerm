use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{
    logging, network_interface::DEFAULT_WILDCARD_BIND_IP, storage::repository::SettingsRepository,
};

pub(crate) const PROXY_STATS_EVENT: &str = "proxy-stats";
pub(crate) const DEFAULT_PROXY_BIND_IP: &str = DEFAULT_WILDCARD_BIND_IP;
pub(crate) const DEFAULT_PROXY_PORT: u16 = 3128;
pub(crate) const PROXY_BIND_IP_KEY: &str = "proxyBindIp";
pub(crate) const PROXY_PORT_KEY: &str = "proxyPort";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyConfig {
    pub bind_ip: String,
    pub port: u16,
    pub running: bool,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            bind_ip: DEFAULT_PROXY_BIND_IP.to_string(),
            port: DEFAULT_PROXY_PORT,
            running: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyStatsSnapshot {
    pub bind_ip: String,
    pub port: u16,
    pub running: bool,
    pub upload_bytes_total: u64,
    pub download_bytes_total: u64,
    pub upload_bytes_per_sec: u64,
    pub download_bytes_per_sec: u64,
}

impl ProxyStatsSnapshot {
    pub(crate) fn idle(config: &ProxyConfig) -> Self {
        Self {
            bind_ip: config.bind_ip.clone(),
            port: config.port,
            running: false,
            upload_bytes_total: 0,
            download_bytes_total: 0,
            upload_bytes_per_sec: 0,
            download_bytes_per_sec: 0,
        }
    }
}

pub(crate) struct ProxyManager {
    pub(crate) config: ProxyConfig,
    pub(crate) runtime: Option<crate::proxy::runtime::ProxyRuntimeHandle>,
}

impl ProxyManager {
    pub(crate) fn from_store(store: &impl SettingsRepository) -> Self {
        let bind_ip = store
            .setting_value(PROXY_BIND_IP_KEY)
            .ok()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_PROXY_BIND_IP.to_string());
        let port = store
            .setting_value(PROXY_PORT_KEY)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_PROXY_PORT);
        Self {
            config: ProxyConfig {
                bind_ip,
                port,
                running: false,
            },
            runtime: None,
        }
    }

    pub(crate) fn config_snapshot(&self) -> ProxyConfig {
        ProxyConfig {
            running: self
                .runtime
                .as_ref()
                .is_some_and(crate::proxy::runtime::ProxyRuntimeHandle::is_running),
            ..self.config.clone()
        }
    }

    pub(crate) fn stats_snapshot(&self) -> ProxyStatsSnapshot {
        match &self.runtime {
            Some(runtime) if runtime.is_running() => runtime.shared.snapshot(&self.config),
            _ => ProxyStatsSnapshot::idle(&self.config),
        }
    }
}

pub(crate) struct ProxySharedState {
    upload_bytes_total: AtomicU64,
    download_bytes_total: AtomicU64,
    upload_bytes_tick: AtomicU64,
    download_bytes_tick: AtomicU64,
    upload_bytes_per_sec: AtomicU64,
    download_bytes_per_sec: AtomicU64,
}

impl ProxySharedState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            upload_bytes_total: AtomicU64::new(0),
            download_bytes_total: AtomicU64::new(0),
            upload_bytes_tick: AtomicU64::new(0),
            download_bytes_tick: AtomicU64::new(0),
            upload_bytes_per_sec: AtomicU64::new(0),
            download_bytes_per_sec: AtomicU64::new(0),
        })
    }

    pub(crate) fn record_upload(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.upload_bytes_total.fetch_add(bytes, Ordering::Relaxed);
        self.upload_bytes_tick.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_download(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.download_bytes_total
            .fetch_add(bytes, Ordering::Relaxed);
        self.download_bytes_tick.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self, config: &ProxyConfig) -> ProxyStatsSnapshot {
        ProxyStatsSnapshot {
            bind_ip: config.bind_ip.clone(),
            port: config.port,
            running: true,
            upload_bytes_total: self.upload_bytes_total.load(Ordering::Relaxed),
            download_bytes_total: self.download_bytes_total.load(Ordering::Relaxed),
            upload_bytes_per_sec: self.upload_bytes_per_sec.load(Ordering::Relaxed),
            download_bytes_per_sec: self.download_bytes_per_sec.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn snapshot_with_window(
        &self,
        config: &ProxyConfig,
        elapsed: Duration,
    ) -> ProxyStatsSnapshot {
        let elapsed_ms = elapsed.as_millis().max(1) as u64;
        let upload_delta = self.upload_bytes_tick.swap(0, Ordering::Relaxed);
        let download_delta = self.download_bytes_tick.swap(0, Ordering::Relaxed);
        let upload_rate = upload_delta.saturating_mul(1_000) / elapsed_ms;
        let download_rate = download_delta.saturating_mul(1_000) / elapsed_ms;
        self.upload_bytes_per_sec
            .store(upload_rate, Ordering::Relaxed);
        self.download_bytes_per_sec
            .store(download_rate, Ordering::Relaxed);

        ProxyStatsSnapshot {
            upload_bytes_per_sec: upload_rate,
            download_bytes_per_sec: download_rate,
            ..self.snapshot(config)
        }
    }
}

pub(crate) fn emit_proxy_stats(app: &AppHandle, snapshot: &ProxyStatsSnapshot) {
    if let Err(error) = app.emit(PROXY_STATS_EVENT, snapshot.clone()) {
        logging::event("proxy.events", "proxy.stats.emit_failed")
            .field("error", error)
            .warn();
    }
}
