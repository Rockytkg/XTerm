use tauri::AppHandle;

use crate::{
    logging,
    network_interface::NetworkInterface,
    proxy::{
        models::{ProxyConfig, ProxyStatsSnapshot},
        service::ProxyService,
    },
    state::AppState,
};

#[tauri::command]
pub(crate) fn get_proxy_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ProxyConfig, String> {
    logging::event("proxy.commands", "proxy.config.get").trace();
    Ok(ProxyService::new(app, state.inner()).config())
}

#[tauri::command]
pub(crate) fn get_proxy_stats(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ProxyStatsSnapshot, String> {
    logging::event("proxy.commands", "proxy.stats.get").trace();
    Ok(ProxyService::new(app, state.inner()).stats())
}

#[tauri::command]
pub(crate) fn get_network_interfaces(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<NetworkInterface>, String> {
    logging::event("proxy.commands", "proxy.interfaces.get").trace();
    ProxyService::new(app, state.inner()).network_interfaces()
}

#[tauri::command]
pub(crate) async fn start_proxy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    port: u16,
    bind_ip: String,
) -> Result<ProxyConfig, String> {
    ProxyService::new(app, state.inner())
        .start(port, bind_ip)
        .await
}

#[tauri::command]
pub(crate) async fn stop_proxy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ProxyConfig, String> {
    ProxyService::new(app, state.inner()).stop().await
}

#[tauri::command]
pub(crate) async fn update_port(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    new_port: u16,
) -> Result<ProxyConfig, String> {
    ProxyService::new(app, state.inner())
        .update_port(new_port)
        .await
}

#[tauri::command]
pub(crate) async fn set_proxy_bind_ip(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bind_ip: String,
) -> Result<ProxyConfig, String> {
    ProxyService::new(app, state.inner())
        .update_bind_ip(bind_ip)
        .await
}
