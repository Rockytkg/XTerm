mod app;
mod app_info;
mod command_registry;
mod credentials;
pub(crate) mod deep_link;
mod elevated;
mod file_service;
mod firewall;
mod fonts;
mod ids;
mod logging;
mod network_interface;
mod paths;
mod proxy;
mod scripting;
mod session_recording;
mod state;
mod storage;
mod terminal;
mod workspace;

pub(crate) fn unix_timestamp_millis() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

pub fn install_early_panic_hook() {
    logging::install_panic_hook();
}

pub fn run() {
    #[cfg(any(windows, unix))]
    if firewall::handle_elevated_helper() {
        return;
    }
    #[cfg(target_os = "linux")]
    if elevated::handle_elevated_bind_helper() {
        return;
    }

    app::run();
}
