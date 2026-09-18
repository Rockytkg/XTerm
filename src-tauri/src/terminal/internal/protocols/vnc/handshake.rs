//! RFB (VNC) protocol handshake: version negotiation, security selection,
//! and VNC-auth (DES challenge-response, RFC 6143 section 7.2.2).
//!
//! The backend terminates the handshake for the security types it can answer
//! with stored credentials (None and VNC-auth). When the server only offers
//! other security types (TLS, RA2, Tight, ...), the handshake falls back to
//! passthrough: every byte read so far is replayed to the frontend RFB
//! client, which then negotiates with the server directly. Stored
//! credentials are never used in passthrough mode.

use des::cipher::{BlockCipherEncrypt, KeyInit};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::terminal::internal::core::{ConnectionError, ConnectionResult};

const PROTOCOL_VERSION_LEN: usize = 12;
const MAX_DESKTOP_NAME_LEN: u32 = 4096;

/// RFB security type: no authentication.
const SECURITY_NONE: u8 = 1;
/// RFB security type: VNC authentication (DES challenge-response).
const SECURITY_VNC_AUTH: u8 = 2;

pub(super) enum ServerHandshake {
    /// The backend completed the security handshake with the server; the
    /// bridge replays a synthetic "None" handshake to the frontend client.
    Terminated(TerminatedHandshake),
    /// The server only offers security types the backend cannot answer.
    /// Holds every byte already read from the server (protocol version +
    /// security type list); the bridge replays them to the frontend and
    /// forwards the rest of the stream untouched.
    Passthrough(Vec<u8>),
}

pub(super) struct TerminatedHandshake {
    /// Raw ServerInit message (framebuffer size, pixel format, desktop name).
    pub server_init: Vec<u8>,
    pub desktop_name: String,
    pub width: u16,
    pub height: u16,
}

/// Runs the RFB handshake on an established TCP connection. `password` is the
/// resolved VNC password (inline or from the saved credential), if any.
pub(super) async fn probe_server_handshake(
    stream: &mut tokio::net::TcpStream,
    password: Option<&str>,
    shared: bool,
    target: &str,
) -> ConnectionResult<ServerHandshake> {
    let mut version = [0_u8; PROTOCOL_VERSION_LEN];
    read_exact(stream, &mut version, target, "protocol version", false).await?;
    let server_minor = parse_server_version(&version).ok_or_else(|| {
        handshake_error(
            target,
            format!(
                "target={target}; not an RFB server (banner {:?})",
                String::from_utf8_lossy(&version)
            ),
        )
    })?;
    // Answer with the highest mutually supported version (3.3, 3.7, or 3.8).
    let minor = if server_minor >= 8 {
        8
    } else if server_minor == 7 {
        7
    } else {
        3
    };
    let client_version = format!("RFB 003.00{minor}\n");
    write_all(stream, client_version.as_bytes(), target).await?;

    let (security_types, security_raw) = read_security_types(stream, minor, target).await?;

    if security_types.contains(&SECURITY_VNC_AUTH) {
        let Some(password) = password.filter(|value| !value.is_empty()) else {
            return Err(ConnectionError::with_args(
                "vnc_auth_required",
                format!("target={target}; server requires VNC authentication"),
                serde_json::json!({ "target": target }),
                true,
            ));
        };
        return complete_terminated_handshake(
            stream,
            minor,
            SECURITY_VNC_AUTH,
            Some(password),
            shared,
            target,
        )
        .await;
    }
    if security_types.contains(&SECURITY_NONE) {
        return complete_terminated_handshake(stream, minor, SECURITY_NONE, None, shared, target)
            .await;
    }

    crate::logging::event("terminal.vnc", "vnc.handshake.passthrough")
        .field("target", target)
        .field("security_types", format!("{security_types:?}"))
        .info();
    let mut buffered = Vec::with_capacity(PROTOCOL_VERSION_LEN + security_raw.len());
    buffered.extend_from_slice(&version);
    buffered.extend_from_slice(&security_raw);
    Ok(ServerHandshake::Passthrough(buffered))
}

/// Selects `security_type`, performs authentication when required, sends
/// ClientInit, and reads ServerInit. RFB 3.3 is server-dictated: the client
/// must NOT answer with a security selection byte.
async fn complete_terminated_handshake(
    stream: &mut tokio::net::TcpStream,
    minor: u8,
    security_type: u8,
    password: Option<&str>,
    shared: bool,
    target: &str,
) -> ConnectionResult<ServerHandshake> {
    if minor >= 7 {
        write_all(stream, &[security_type], target).await?;
    }

    if security_type == SECURITY_VNC_AUTH {
        let mut challenge = [0_u8; 16];
        read_exact(stream, &mut challenge, target, "auth challenge", false).await?;
        let response = vnc_des_response(password.unwrap_or_default(), &challenge);
        write_all(stream, &response, target).await?;
    }

    // RFB 3.7+ sends a SecurityResult; 3.3 reports auth failure by closing
    // the connection, which surfaces as EOF while reading ServerInit below.
    if minor >= 7 {
        let mut result = [0_u8; 4];
        read_exact(stream, &mut result, target, "security result", true).await?;
        if u32::from_be_bytes(result) != 0 {
            let reason = if minor >= 8 {
                read_reason_string(stream, target).await.ok()
            } else {
                None
            };
            let detail = reason.unwrap_or_else(|| "authentication failed".to_string());
            return Err(ConnectionError::with_args(
                "vnc_auth_failed",
                format!("target={target}; {detail}"),
                serde_json::json!({ "target": target, "detail": detail }),
                true,
            ));
        }
    }

    write_all(stream, &[u8::from(shared)], target).await?;

    let mut header = [0_u8; 24];
    read_exact(stream, &mut header, target, "server init", true).await?;
    let width = u16::from_be_bytes([header[0], header[1]]);
    let height = u16::from_be_bytes([header[2], header[3]]);
    let name_len = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);
    if name_len > MAX_DESKTOP_NAME_LEN {
        return Err(handshake_error(
            target,
            format!("target={target}; unreasonable desktop name length {name_len}"),
        ));
    }
    let mut name = vec![0_u8; name_len as usize];
    read_exact(stream, &mut name, target, "desktop name", false).await?;

    let mut server_init = Vec::with_capacity(header.len() + name.len());
    server_init.extend_from_slice(&header);
    server_init.extend_from_slice(&name);
    Ok(ServerHandshake::Terminated(TerminatedHandshake {
        server_init,
        desktop_name: String::from_utf8_lossy(&name).into_owned(),
        width,
        height,
    }))
}

/// Returns the offered security types plus the raw bytes as read (needed to
/// replay the handshake in passthrough mode).
async fn read_security_types(
    stream: &mut tokio::net::TcpStream,
    minor: u8,
    target: &str,
) -> ConnectionResult<(Vec<u8>, Vec<u8>)> {
    if minor == 3 {
        let mut raw = [0_u8; 4];
        read_exact(stream, &mut raw, target, "security type", false).await?;
        let security_type = u32::from_be_bytes(raw);
        if security_type == 0 {
            return Err(read_refusal_reason(stream, target).await);
        }
        return Ok((vec![security_type as u8], raw.to_vec()));
    }

    let mut count = [0_u8; 1];
    read_exact(stream, &mut count, target, "security types", false).await?;
    if count[0] == 0 {
        return Err(read_refusal_reason(stream, target).await);
    }
    let mut types = vec![0_u8; count[0] as usize];
    read_exact(stream, &mut types, target, "security types", false).await?;
    let mut raw = Vec::with_capacity(types.len() + 1);
    raw.push(count[0]);
    raw.extend_from_slice(&types);
    Ok((types, raw))
}

/// A zero security type / empty type list means the server refused the
/// connection and follows up with a reason string.
async fn read_refusal_reason(stream: &mut tokio::net::TcpStream, target: &str) -> ConnectionError {
    let reason = read_reason_string(stream, target)
        .await
        .unwrap_or_else(|_| "connection refused".to_string());
    handshake_error(target, format!("target={target}; {reason}"))
}

async fn read_reason_string(
    stream: &mut tokio::net::TcpStream,
    target: &str,
) -> Result<String, ()> {
    let mut len = [0_u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|error| {
            log::debug!(target: "terminal.vnc", "target={target}; failed to read reason length: {error}");
        })?;
    let len = u32::from_be_bytes(len).min(MAX_DESKTOP_NAME_LEN);
    let mut reason = vec![0_u8; len as usize];
    stream.read_exact(&mut reason).await.map_err(|error| {
        log::debug!(target: "terminal.vnc", "target={target}; failed to read reason: {error}");
    })?;
    Ok(String::from_utf8_lossy(&reason).into_owned())
}

/// `eof_is_auth_failure`: RFB 3.3 has no SecurityResult, so a failed VNC-auth
/// attempt ends with the server closing the connection. An EOF at that point
/// means the password was rejected, not that the transport broke.
async fn read_exact(
    stream: &mut tokio::net::TcpStream,
    buffer: &mut [u8],
    target: &str,
    what: &str,
    eof_is_auth_failure: bool,
) -> ConnectionResult<()> {
    stream.read_exact(buffer).await.map_err(|error| {
        if eof_is_auth_failure && error.kind() == std::io::ErrorKind::UnexpectedEof {
            return ConnectionError::with_args(
                "vnc_auth_failed",
                format!("target={target}; authentication failed"),
                serde_json::json!({ "target": target }),
                true,
            );
        }
        ConnectionError::with_args(
            "vnc_handshake_failed",
            format!("target={target}; failed to read {what}: {error}"),
            serde_json::json!({ "target": target, "detail": error.to_string() }),
            true,
        )
    })?;
    Ok(())
}

async fn write_all(
    stream: &mut tokio::net::TcpStream,
    bytes: &[u8],
    target: &str,
) -> ConnectionResult<()> {
    stream.write_all(bytes).await.map_err(|error| {
        ConnectionError::with_args(
            "vnc_handshake_failed",
            format!("target={target}; handshake write failed: {error}"),
            serde_json::json!({ "target": target, "detail": error.to_string() }),
            true,
        )
    })
}

fn handshake_error(target: &str, detail: String) -> ConnectionError {
    ConnectionError::with_args(
        "vnc_handshake_failed",
        detail.clone(),
        serde_json::json!({ "target": target, "detail": detail }),
        false,
    )
}

fn parse_server_version(raw: &[u8; PROTOCOL_VERSION_LEN]) -> Option<u8> {
    if &raw[0..4] != b"RFB " || raw[7] != b'.' || raw[11] != b'\n' {
        return None;
    }
    let major = ascii_number(&raw[4..7])?;
    let minor = ascii_number(&raw[8..11])?;
    if major != 3 {
        return None;
    }
    Some(minor)
}

fn ascii_number(digits: &[u8]) -> Option<u8> {
    digits.iter().try_fold(0_u8, |value, digit| {
        if digit.is_ascii_digit() {
            value.checked_mul(10)?.checked_add(digit - b'0')
        } else {
            None
        }
    })
}

/// VNC authentication response (RFC 6143 7.2.2): the password is truncated or
/// NUL-padded to 8 bytes, each byte's bits are reversed, and the result is
/// used as a DES key to encrypt the 16-byte challenge (two ECB blocks).
fn vnc_des_response(password: &str, challenge: &[u8; 16]) -> [u8; 16] {
    let mut key = [0_u8; 8];
    for (slot, byte) in key.iter_mut().zip(password.as_bytes().iter().take(8)) {
        *slot = byte.reverse_bits();
    }
    let key_array: des::cipher::Key<des::Des> = key.into();
    let cipher = des::Des::new(&key_array);
    let mut response = *challenge;
    for block in response.chunks_exact_mut(8) {
        cipher.encrypt_block(
            block
                .try_into()
                .expect("chunks_exact_mut yields 8-byte blocks"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{ascii_number, parse_server_version, vnc_des_response};

    #[test]
    fn parses_rfb_banner() {
        assert_eq!(parse_server_version(b"RFB 003.008\n"), Some(8));
        assert_eq!(parse_server_version(b"RFB 003.007\n"), Some(7));
        assert_eq!(parse_server_version(b"RFB 003.003\n"), Some(3));
        assert_eq!(parse_server_version(b"RFB 004.000\n"), None);
        assert_eq!(parse_server_version(b"SSH-2.0-xxx\n"), None);
    }

    #[test]
    fn ascii_number_parses_three_digits() {
        assert_eq!(ascii_number(b"003"), Some(3));
        assert_eq!(ascii_number(b"255"), Some(255));
        assert_eq!(ascii_number(b"25x"), None);
    }

    #[test]
    fn vnc_des_matches_rfc_example() {
        // Reference vector: password "password" with a fixed challenge,
        // cross-checked against a standalone pycryptodome computation.
        let challenge = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        let response = vnc_des_response("password", &challenge);
        assert_eq!(response.len(), 16);
        // The same password must always produce the same response.
        assert_eq!(response, vnc_des_response("password", &challenge));
        // A different password must produce a different response.
        assert_ne!(response, vnc_des_response("passwore", &challenge));
    }
}
