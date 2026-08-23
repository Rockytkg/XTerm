use tauri::AppHandle;

use crate::firewall::{remove_service_ports_rule, FirewallCommandError, FirewallProtocol};

const TFTP_RULE_PREFIX: &str = "XTerm TFTP";
const FTP_RULE_PREFIX: &str = "XTerm FTP";

pub(crate) async fn remove_tftp_port_rule<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    port: u16,
) -> Result<(), FirewallCommandError> {
    crate::firewall::remove_service_port_and_all_udp_ports_for_current_app_rule(
        TFTP_RULE_PREFIX,
        "tftp.firewall.remove",
        port,
    )
    .await
}

pub(crate) async fn remove_ftp_ports(
    control_port: u16,
    passive_ports: std::ops::RangeInclusive<u16>,
) -> Result<(), String> {
    let ports = std::iter::once(control_port)
        .chain(passive_ports)
        .collect::<Vec<_>>();
    remove_service_ports_rule(
        FTP_RULE_PREFIX,
        "ftp.firewall.remove",
        ports,
        FirewallProtocol::Tcp,
    )
    .await
    .map_err(|error| error.user_message)
}
