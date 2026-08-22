use tauri::AppHandle;

use crate::{
    network_interface::{resolve_network_interfaces, validate_bind_ip, NetworkInterface},
    proxy::{
        models::{
            emit_proxy_stats, ProxyConfig, ProxyStatsSnapshot, PROXY_BIND_IP_KEY, PROXY_PORT_KEY,
        },
        runtime::{start_runtime, stop_runtime},
    },
    state::AppState,
    storage::repository::SettingsRepository,
};

pub(crate) struct ProxyService<'a> {
    app: AppHandle,
    state: &'a AppState,
}

impl<'a> ProxyService<'a> {
    pub(crate) fn new(app: AppHandle, state: &'a AppState) -> Self {
        Self { app, state }
    }

    pub(crate) fn config(&self) -> ProxyConfig {
        self.state.proxy().config_snapshot()
    }

    pub(crate) fn stats(&self) -> ProxyStatsSnapshot {
        self.state.proxy().stats_snapshot()
    }

    pub(crate) fn network_interfaces(&self) -> Result<Vec<NetworkInterface>, String> {
        resolve_network_interfaces()
    }

    pub(crate) async fn start(&self, port: u16, bind_ip: String) -> Result<ProxyConfig, String> {
        self.apply(port, bind_ip).await
    }

    pub(crate) async fn stop(&self) -> Result<ProxyConfig, String> {
        self.stop_runtime().await
    }

    pub(crate) async fn update_port(&self, new_port: u16) -> Result<ProxyConfig, String> {
        validate_port(new_port)?;
        let (bind_ip, was_running, old_port) = {
            let manager = self.state.proxy();
            (
                manager.config.bind_ip.clone(),
                manager.runtime.is_some(),
                manager.config.port,
            )
        };

        self.persist_settings(Some(new_port), None)?;
        crate::logging::event("proxy.commands", "proxy.port.update")
            .field("old_port", old_port)
            .field("new_port", new_port)
            .info();

        if !was_running {
            let snapshot = {
                let mut manager = self.state.proxy();
                manager.config.port = new_port;
                manager.config_snapshot()
            };
            return Ok(snapshot);
        }

        self.restart(new_port, bind_ip).await
    }

    pub(crate) async fn update_bind_ip(&self, bind_ip: String) -> Result<ProxyConfig, String> {
        validate_bind_ip(&bind_ip)?;
        self.persist_settings(None, Some(&bind_ip))?;

        let (port, was_running) = {
            let mut manager = self.state.proxy();
            manager.config.bind_ip = bind_ip.clone();
            (manager.config.port, manager.runtime.is_some())
        };

        if !was_running {
            return Ok(self.state.proxy().config_snapshot());
        }

        self.restart(port, bind_ip).await
    }

    async fn restart(&self, port: u16, bind_ip: String) -> Result<ProxyConfig, String> {
        let _ = self.stop_runtime().await;
        self.apply(port, bind_ip).await
    }

    async fn apply(&self, port: u16, bind_ip: String) -> Result<ProxyConfig, String> {
        validate_port(port)?;
        validate_bind_ip(&bind_ip)?;
        self.persist_settings(Some(port), Some(&bind_ip))?;

        if self.state.proxy().runtime.is_some() {
            let _ = self.stop_runtime().await;
        }

        let runtime = match start_runtime(self.app.clone(), port, bind_ip.clone()).await {
            Ok(runtime) => runtime,
            Err(error) => {
                let mut manager = self.state.proxy();
                manager.config.bind_ip = bind_ip;
                manager.config.port = port;
                emit_proxy_stats(&self.app, &manager.stats_snapshot());
                return Err(error);
            }
        };

        let snapshot = {
            let mut manager = self.state.proxy();
            manager.config.bind_ip = bind_ip;
            manager.config.port = port;
            manager.runtime = Some(runtime);
            manager.config_snapshot()
        };
        Ok(snapshot)
    }

    async fn stop_runtime(&self) -> Result<ProxyConfig, String> {
        let (runtime, bind_ip, port) = {
            let mut manager = self.state.proxy();
            let runtime = manager.runtime.take();
            let bind_ip = manager.config.bind_ip.clone();
            let port = manager.config.port;
            (runtime, bind_ip, port)
        };

        if let Some(runtime) = runtime {
            stop_runtime(runtime, port).await?;
        }

        let snapshot = ProxyConfig {
            bind_ip,
            port,
            running: false,
        };
        emit_proxy_stats(&self.app, &ProxyStatsSnapshot::idle(&snapshot));
        Ok(snapshot)
    }

    fn persist_settings(&self, port: Option<u16>, bind_ip: Option<&str>) -> Result<(), String> {
        let store = self.state.store();
        if let Some(port) = port {
            SettingsRepository::set_setting(&*store, PROXY_PORT_KEY, &port.to_string())?;
        }
        if let Some(bind_ip) = bind_ip {
            SettingsRepository::set_setting(&*store, PROXY_BIND_IP_KEY, bind_ip)?;
        }
        Ok(())
    }
}

fn validate_port(port: u16) -> Result<(), String> {
    if port == 0 {
        return Err("The proxy port must be between 1 and 65535.".to_string());
    }
    Ok(())
}
