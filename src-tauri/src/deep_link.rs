//! System-level default-app deep-link support for `ssh://` and `telnet://`
//! IETF-standard URIs.  Scheme registration is delegated to
//! `tauri-plugin-deep-link` — no per-platform registry manipulation needed.
//!
//! ## URI format
//! - `ssh://[user[:password]@]host[:port]`   (draft-ietf-secsh-scp-sftp-ssh-uri)
//! - `telnet://[user[:password]@]host[:port]` (RFC 4248)

use crate::{
    ids, logging,
    state::AppState,
    terminal::{domain::ProtocolKind, internal::ResolvedConnection},
};
use serde::Serialize;

// ── URI parsing ─────────────────────────────────────────────────────────────

/// Compact parser for `ssh://` and `telnet://` URIs.  Single forward scan,
/// zero-copy slice extraction, lazy percent-decode.
fn parse_uri(uri: &str) -> Option<ParsedUri> {
    let b = uri.as_bytes();

    // Detect scheme via byte-level prefix
    let (scheme, prefix_len) = if b.len() >= 6 && b[..6] == *b"ssh://" {
        (Scheme::Ssh, 6usize)
    } else if b.len() >= 9 && b[..9] == *b"telnet://" {
        (Scheme::Telnet, 9)
    } else {
        return None;
    };

    let authority = &uri[prefix_len..];
    let authority = match authority.find(['/', '?', '#']) {
        Some(pos) => &authority[..pos],
        None => authority,
    };

    // userinfo@hostport
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(pos) => (Some(&authority[..pos]), &authority[pos + 1..]),
        None => (None, authority),
    };

    // user[:password]
    let (user, password) = match userinfo {
        Some(raw) => match raw.find(':') {
            Some(colon) => (
                non_empty(decode(&raw[..colon])),
                non_empty(decode(&raw[colon + 1..])),
            ),
            None => (non_empty(decode(raw)), None),
        },
        None => (None, None),
    };

    // host[:port] (IPv6 bracket-aware)
    let (host_raw, port) = if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(close) => {
                let h = &hostport[..=close];
                let port = hostport[close + 1..]
                    .strip_prefix(':')
                    .and_then(|p| p.parse::<u16>().ok())
                    .unwrap_or(scheme.default_port());
                (h, port)
            }
            None => (hostport, scheme.default_port()),
        }
    } else {
        match hostport.rfind(':') {
            Some(pos) => {
                let port = hostport[pos + 1..].parse::<u16>().ok();
                (&hostport[..pos], port.unwrap_or(scheme.default_port()))
            }
            None => (hostport, scheme.default_port()),
        }
    };

    let host = decode(host_raw);
    if host.is_empty() {
        return None;
    }

    Some(ParsedUri {
        scheme,
        host,
        port,
        user,
        password,
    })
}

#[inline]
fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Percent-decode.  Most terminal URIs contain no encoded characters,
/// so we check first to skip allocation entirely.
#[inline]
fn decode(raw: &str) -> String {
    if !raw.contains('%') && !raw.contains('+') {
        return raw.to_owned();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let hi = chars.next().and_then(hex);
                let lo = chars.next().and_then(hex);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4 | lo) as char);
                }
            }
            b'+' => out.push(' '),
            _ => out.push(b as char),
        }
    }
    out
}

#[inline]
fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' | b'a'..=b'f' => Some((b & 0x0F) + 9),
        _ => None,
    }
}

// ── Internal types ──────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Scheme {
    Ssh,
    Telnet,
}

impl Scheme {
    #[inline]
    const fn default_port(self) -> u16 {
        match self {
            Self::Ssh => 22,
            Self::Telnet => 23,
        }
    }

    fn to_protocol_kind(self) -> ProtocolKind {
        match self {
            Self::Ssh => ProtocolKind::Ssh,
            Self::Telnet => ProtocolKind::Telnet,
        }
    }
}

struct ParsedUri {
    scheme: Scheme,
    host: String,
    port: u16,
    user: Option<String>,
    password: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepLinkEndpoint {
    protocol: ProtocolKind,
    host: String,
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
}

impl ParsedUri {
    fn into_resolved(self, id: String) -> ResolvedConnection {
        ResolvedConnection {
            id,
            open_request_id: None,
            open_scope: None,
            protocol: self.scheme.to_protocol_kind(),
            host: Some(self.host),
            port: Some(self.port),
            user: self.user,
            serial_port: None,
            baud_rate: None,
            serial_quick_auto_baud: None,
            data_bits: None,
            flow_control: None,
            parity: None,
            stop_bits: None,
            encoding: None,
            realtime_encoding_detection: None,
            auth_method: None,
            saved_credential_id: None,
            inline_password: self.password,
            inline_private_key: None,
            inline_private_key_passphrase: None,
            trust_host_key: None,
            accept_host_key_once: None,
            terminal_scrollback: None,
            terminal_type: None,
            runtime_metrics: None,
            cols: None,
            rows: None,
            jump_hosts: None,
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub(crate) fn terminal_resolve_uri(
    state: tauri::State<'_, AppState>,
    uri: String,
) -> Result<DeepLinkResolvedConnection, String> {
    let parsed = parse_uri(&uri).ok_or_else(|| format!("unsupported or malformed URI: {uri}"))?;
    let id = ids::new_id();
    let protocol = parsed.scheme.to_protocol_kind();
    let endpoint = DeepLinkEndpoint {
        protocol,
        host: parsed.host.clone(),
        port: parsed.port,
        user: parsed.user.clone(),
    };

    logging::event("deep_link", "terminal.open.uri")
        .field("connection_id", &id)
        .field("protocol", protocol.as_str())
        .field("host", format_args!("{}:{}", parsed.host, parsed.port))
        .field("has_password", parsed.password.is_some())
        .info();

    let resolved = parsed.into_resolved(id.clone());
    state.remember_transient_connection(resolved);

    Ok(DeepLinkResolvedConnection {
        connection_id: id,
        name: format_endpoint_name(endpoint.user.as_deref(), &endpoint.host, endpoint.port),
        protocol,
        endpoint,
        transient: true,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeepLinkResolvedConnection {
    connection_id: String,
    endpoint: DeepLinkEndpoint,
    name: String,
    protocol: ProtocolKind,
    transient: bool,
}

fn format_endpoint_name(user: Option<&str>, host: &str, port: u16) -> String {
    let authority = match user.filter(|value| !value.is_empty()) {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    };
    format!("{authority}:{port}")
}
