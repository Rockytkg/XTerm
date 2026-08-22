use tauri::AppHandle;

use crate::firewall::{
    allow_service_port, allow_service_ports, remove_service_port_rule, remove_service_ports_rule,
    FirewallCommandError, FirewallProtocol,
};

const TFTP_RULE_PREFIX: &str = "XTerm TFTP";
const SFTP_RULE_PREFIX: &str = "XTerm SFTP";
const FTP_RULE_PREFIX: &str = "XTerm FTP";

pub(crate) async fn allow_tftp_port<R: tauri::Runtime>(
    _app: &AppHandle<R>,
    port: u16,
) -> Result<(), FirewallCommandError> {
    crate::firewall::allow_service_port_and_all_udp_ports_for_current_app(
        TFTP_RULE_PREFIX,
        "tftp.firewall.allow",
        port,
    )
    .await
}

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

pub(crate) async fn allow_sftp_port(port: u16) -> Result<(), FirewallCommandError> {
    allow_service_port(
        SFTP_RULE_PREFIX,
        "sftp.firewall.allow",
        port,
        FirewallProtocol::Tcp,
    )
    .await
}

pub(crate) async fn remove_sftp_port_rule(port: u16) -> Result<(), FirewallCommandError> {
    remove_service_port_rule(
        SFTP_RULE_PREFIX,
        "sftp.firewall.remove",
        port,
        FirewallProtocol::Tcp,
    )
    .await
}

pub(crate) async fn allow_ftp_ports(
    control_port: u16,
    passive_ports: std::ops::RangeInclusive<u16>,
) -> Result<(), String> {
    let ports = std::iter::once(control_port)
        .chain(passive_ports)
        .collect::<Vec<_>>();
    allow_service_ports(
        FTP_RULE_PREFIX,
        "ftp.firewall.allow",
        ports,
        FirewallProtocol::Tcp,
    )
    .await
    .map_err(|error| error.user_message)
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
