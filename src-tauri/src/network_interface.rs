use std::net::IpAddr;

use if_addrs::get_if_addrs;
use serde::Serialize;

pub(crate) const DEFAULT_WILDCARD_BIND_IP: &str = "0.0.0.0";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkInterface {
    pub name: String,
    pub label: String,
    pub ip: String,
    pub is_loopback: bool,
    pub is_wildcard: bool,
}

pub(crate) fn resolve_network_interfaces() -> Result<Vec<NetworkInterface>, String> {
    let mut interfaces = vec![NetworkInterface {
        name: "all".to_string(),
        label: format!("All interfaces ({DEFAULT_WILDCARD_BIND_IP})"),
        ip: DEFAULT_WILDCARD_BIND_IP.to_string(),
        is_loopback: false,
        is_wildcard: true,
    }];

    for interface in get_if_addrs().map_err(|error| format!("failed to enumerate NICs: {error}"))? {
        let ip = interface.ip();
        if ip.is_unspecified() {
            continue;
        }
        interfaces.push(NetworkInterface {
            label: format!("{} ({ip})", interface.name),
            name: interface.name,
            ip: ip.to_string(),
            is_loopback: ip.is_loopback(),
            is_wildcard: false,
        });
    }

    interfaces.sort_by(|left, right| {
        left.is_wildcard
            .cmp(&right.is_wildcard)
            .reverse()
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.ip.cmp(&right.ip))
    });

    Ok(interfaces)
}

pub(crate) fn validate_bind_ip(bind_ip: &str) -> Result<(), String> {
    let normalized = bind_ip.trim();
    if normalized.eq(DEFAULT_WILDCARD_BIND_IP) {
        return Ok(());
    }

    let ip = normalized
        .parse::<IpAddr>()
        .map_err(|_| format!("invalid bind IP address '{bind_ip}'"))?;
    // NIC presence is advisory only: a saved config must keep loading when
    // its interface is temporarily gone (cable unplugged, VPN down), and the
    // real bind() reports the honest OS error when the address is truly
    // unusable.  Compared by value so non-canonical forms (0::1) still match.
    match resolve_network_interfaces() {
        Ok(interfaces)
            if interfaces
                .iter()
                .any(|interface| interface.ip.parse::<IpAddr>() == Ok(ip)) => {}
        Ok(_) => {
            crate::logging::event("network_interface", "bind_ip.not_on_nic")
                .field("bind_ip", normalized)
                .warn();
        }
        Err(error) => {
            crate::logging::event("network_interface", "bind_ip.enum_failed")
                .field("bind_ip", normalized)
                .field("error", &error)
                .warn();
        }
    }
    Ok(())
}
