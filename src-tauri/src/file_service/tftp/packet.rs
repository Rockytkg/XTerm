//! Pure TFTP wire codec: request parsing, option negotiation (RFC 2347/2348/
//! 2349/7440), packet encoding/decoding and netascii conversion.  No socket
//! or filesystem access happens here, so everything is unit-testable.

use std::time::Duration;

use super::error::TransferError;

pub(super) const TFTP_DEFAULT_BLOCK_SIZE: usize = 512;
pub(super) const TFTP_MAX_BLOCK_SIZE: usize = 1468;
pub(super) const TFTP_MAX_WINDOW_SIZE: usize = 64;
/// Receive buffer: header + largest negotiated block + slack for options.
pub(super) const MAX_PACKET_SIZE: usize = 4 + TFTP_MAX_BLOCK_SIZE + 1024;
/// RFC 1350 caps RRQ/WRQ packets at 512 bytes even with options present.
const MAX_REQUEST_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Opcode {
    Read = 1,
    Write = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TransferMode {
    Octet,
    Netascii,
}

#[derive(Debug)]
pub(super) struct Request {
    pub(super) opcode: Opcode,
    pub(super) filename: String,
    pub(super) mode: TransferMode,
    pub(super) options: Vec<(String, String)>,
}

#[derive(Debug)]
pub(super) struct Negotiated {
    pub(super) block_size: usize,
    pub(super) window_size: usize,
    pub(super) timeout: Duration,
    pub(super) oack: Vec<(String, String)>,
}

/// A packet received during an established transfer (i.e. not RRQ/WRQ).
pub(super) enum TransferPacket<'a> {
    Data { block: u16, payload: &'a [u8] },
    Ack(u16),
    Error { code: u16, message: String },
}

fn be16(packet: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([packet[offset], packet[offset + 1]])
}

pub(super) fn parse_request(packet: &[u8]) -> Result<Request, TransferError> {
    if packet.len() < 4 {
        return Err(TransferError::illegal("request is too short"));
    }
    let opcode = match be16(packet, 0) {
        1 => Opcode::Read,
        2 => Opcode::Write,
        value => {
            return Err(TransferError::illegal(format!(
                "unsupported opcode {value}"
            )))
        }
    };
    if packet.len() > MAX_REQUEST_SIZE || !packet.ends_with(&[0]) {
        return Err(TransferError::illegal("malformed request"));
    }
    let fields = packet[2..packet.len() - 1]
        .split(|byte| *byte == 0)
        .collect::<Vec<_>>();
    if fields.len() < 2 || fields.iter().any(|field| field.is_empty()) {
        return Err(TransferError::illegal("malformed request"));
    }
    let filename = ascii_field(fields[0], "filename")?;
    let mode = match ascii_field(fields[1], "mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "octet" => TransferMode::Octet,
        "netascii" => TransferMode::Netascii,
        "mail" => {
            return Err(TransferError::illegal(
                "mail transfer mode is obsolete and unsupported",
            ))
        }
        _ => return Err(TransferError::illegal("unsupported transfer mode")),
    };
    let mut options: Vec<(String, String)> = Vec::new();
    let mut index = 2;
    if (fields.len() - 2) % 2 != 0 {
        return Err(TransferError::illegal("malformed option list"));
    }
    while index < fields.len() {
        let key = ascii_field(fields[index], "option")?;
        let value = ascii_field(fields[index + 1], "option value")?;
        if options
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(&key))
        {
            return Err(TransferError::illegal("duplicate option"));
        }
        options.push((key.to_ascii_lowercase(), value));
        index += 2;
    }
    Ok(Request {
        opcode,
        filename,
        mode,
        options,
    })
}

/// Validates the requested RFC 2347 options and builds the OACK answer.
/// Unknown options are ignored (not acknowledged), per RFC 2347 §4.
pub(super) fn negotiate(
    request: &Request,
    transfer_size: u64,
    read: bool,
) -> Result<Negotiated, TransferError> {
    let mut block_size = TFTP_DEFAULT_BLOCK_SIZE;
    let mut timeout_seconds = 3u64;
    let mut window_size = 1usize;
    let mut oack = Vec::new();
    for (key, value) in &request.options {
        match key.as_str() {
            "blksize" => {
                let value = value
                    .parse::<usize>()
                    .map_err(|_| TransferError::bad_option("invalid blksize option"))?;
                if !(8..=65464).contains(&value) {
                    return Err(TransferError::bad_option("invalid blksize option"));
                }
                // The server may acknowledge a smaller value than requested.
                block_size = value.min(TFTP_MAX_BLOCK_SIZE);
                oack.push(("blksize".to_string(), block_size.to_string()));
            }
            "timeout" => {
                timeout_seconds = value
                    .parse::<u64>()
                    .map_err(|_| TransferError::bad_option("invalid timeout option"))?;
                if !(1..=255).contains(&timeout_seconds) {
                    return Err(TransferError::bad_option("invalid timeout option"));
                }
                oack.push(("timeout".to_string(), timeout_seconds.to_string()));
            }
            "tsize" => {
                let requested = value
                    .parse::<u64>()
                    .map_err(|_| TransferError::bad_option("invalid tsize option"))?;
                if read && requested != 0 {
                    return Err(TransferError::bad_option("RRQ tsize must be zero"));
                }
                let value = if read { transfer_size } else { requested };
                oack.push(("tsize".to_string(), value.to_string()));
            }
            "windowsize" => {
                let requested = value
                    .parse::<usize>()
                    .map_err(|_| TransferError::bad_option("invalid windowsize option"))?;
                if !(1..=65535).contains(&requested) {
                    return Err(TransferError::bad_option("invalid windowsize option"));
                }
                window_size = requested.min(TFTP_MAX_WINDOW_SIZE);
                oack.push(("windowsize".to_string(), window_size.to_string()));
            }
            _ => {}
        }
    }
    Ok(Negotiated {
        block_size,
        window_size,
        timeout: Duration::from_secs(timeout_seconds),
        oack,
    })
}

/// The tsize a client announced in its WRQ, if any (0 means unknown).
pub(super) fn request_tsize(request: &Request) -> u64 {
    request
        .options
        .iter()
        .find(|(key, _)| key == "tsize")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0)
}

pub(super) fn decode_transfer_packet(packet: &[u8]) -> Result<TransferPacket<'_>, TransferError> {
    if packet.len() < 4 {
        return Err(TransferError::illegal("packet is too short"));
    }
    match be16(packet, 0) {
        3 => Ok(TransferPacket::Data {
            block: be16(packet, 2),
            payload: &packet[4..],
        }),
        // A few embedded TFTP clients pad the fixed four-byte ACK to an
        // aligned UDP payload. Accept only zero padding for interoperability.
        4 if packet.len() >= 4 && packet[4..].iter().all(|byte| *byte == 0) => {
            Ok(TransferPacket::Ack(be16(packet, 2)))
        }
        4 => Err(TransferError::illegal("malformed ACK packet")),
        5 => {
            if packet.len() < 5 || !packet.ends_with(&[0]) {
                return Err(TransferError::illegal("malformed ERROR packet"));
            }
            Ok(TransferPacket::Error {
                code: be16(packet, 2),
                message: String::from_utf8_lossy(&packet[4..packet.len() - 1]).into_owned(),
            })
        }
        opcode => Err(TransferError::illegal(format!(
            "unexpected opcode {opcode} during transfer"
        ))),
    }
}

pub(super) fn data_packet(block: u16, data: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(4 + data.len());
    packet.extend_from_slice(&[0, 3]);
    packet.extend_from_slice(&block.to_be_bytes());
    packet.extend_from_slice(data);
    packet
}

pub(super) fn ack_packet(block: u16) -> [u8; 4] {
    [0, 4, (block >> 8) as u8, block as u8]
}

pub(super) fn oack_packet(options: &[(String, String)]) -> Vec<u8> {
    let mut packet = vec![0, 6];
    for (key, value) in options {
        packet.extend_from_slice(key.as_bytes());
        packet.push(0);
        packet.extend_from_slice(value.as_bytes());
        packet.push(0);
    }
    packet
}

pub(super) fn error_packet(code: u16, message: &str) -> Vec<u8> {
    let mut packet = vec![0, 5];
    packet.extend_from_slice(&code.to_be_bytes());
    packet.extend_from_slice(message.as_bytes());
    packet.push(0);
    packet
}

fn ascii_field(bytes: &[u8], name: &str) -> Result<String, TransferError> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii) {
        return Err(TransferError::illegal(format!(
            "{name} is not a non-empty netascii string"
        )));
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Converts the local file to netascii (RFC 764): LF -> CRLF, CR -> CR NUL.
pub(super) fn encode_netascii(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    for byte in input {
        match byte {
            b'\n' => output.extend_from_slice(b"\r\n"),
            b'\r' => output.extend_from_slice(b"\r\0"),
            byte => output.push(*byte),
        }
    }
    output
}

/// Converts received netascii back to the local representation.  `pending_cr`
/// carries a CR split across two DATA blocks between calls.
pub(super) fn decode_netascii(input: &[u8], pending_cr: &mut bool) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    for byte in input {
        if *pending_cr {
            match byte {
                b'\n' => output.push(b'\n'),
                0 => output.push(b'\r'),
                byte => {
                    output.push(b'\r');
                    output.push(*byte);
                }
            }
            *pending_cr = false;
        } else if *byte == b'\r' {
            *pending_cr = true;
        } else {
            output.push(*byte);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        data_packet, decode_transfer_packet, encode_netascii, negotiate, parse_request, Opcode,
        TransferPacket, TFTP_MAX_BLOCK_SIZE,
    };

    #[test]
    fn parses_rfc1350_wrq_and_rfc2349_options() {
        let request = parse_request(&[
            0, 2, b'a', b'a', 0, b'o', b'c', b't', b'e', b't', 0, b't', b's', b'i', b'z', b'e', 0,
            b'0', 0, b't', b'i', b'm', b'e', b'o', b'u', b't', 0, b'3', 0,
        ])
        .unwrap();
        assert_eq!(request.opcode, Opcode::Write);
        assert_eq!(request.filename, "aa");
        assert_eq!(request.options[0], ("tsize".to_string(), "0".to_string()));
        assert_eq!(request.options[1], ("timeout".to_string(), "3".to_string()));
    }

    #[test]
    fn negotiates_safe_block_size_and_echoes_write_tsize() {
        let request = parse_request(&[
            0, 2, b'a', b'a', 0, b'o', b'c', b't', b'e', b't', 0, b'b', b'l', b'k', b's', b'i',
            b'z', b'e', 0, b'6', b'5', b'4', b'6', b'4', 0, b't', b's', b'i', b'z', b'e', 0, b'1',
            b'2', b'3', 0,
        ])
        .unwrap();
        let options = negotiate(&request, 123, false).unwrap();
        assert_eq!(options.block_size, TFTP_MAX_BLOCK_SIZE);
        assert!(options
            .oack
            .iter()
            .any(|(key, value)| key == "tsize" && value == "123"));
    }

    #[test]
    fn negotiates_rfc2349_write_tsize_and_rfc7440_window() {
        let request = parse_request(&[
            0, 2, b'a', 0, b'o', b'c', b't', b'e', b't', 0, b't', b's', b'i', b'z', b'e', 0, b'4',
            b'2', 0, b'w', b'i', b'n', b'd', b'o', b'w', b's', b'i', b'z', b'e', 0, b'8', 0,
        ])
        .unwrap();
        let options = negotiate(&request, 0, false).unwrap();
        assert_eq!(options.window_size, 8);
        assert!(options
            .oack
            .iter()
            .any(|option| option == &("tsize".into(), "42".into())));
        assert_eq!(super::oack_packet(&options.oack)[..2], [0, 6]);
    }

    #[test]
    fn packet_codec_validates_rfc_packet_headers() {
        let packet = data_packet(7, b"abc");
        match decode_transfer_packet(&packet).unwrap() {
            TransferPacket::Data { block, payload } => {
                assert_eq!(block, 7);
                assert_eq!(payload, b"abc");
            }
            _ => panic!("expected DATA packet"),
        }
        match decode_transfer_packet(&[0, 4, 0, 7]).unwrap() {
            TransferPacket::Ack(block) => assert_eq!(block, 7),
            _ => panic!("expected ACK packet"),
        }
        // Non-zero trailing garbage on an ACK is rejected.
        assert!(decode_transfer_packet(&[0, 4, 0, 7, 1]).is_err());
        assert!(matches!(
            decode_transfer_packet(&[0, 4, 0, 7, 0, 0]).unwrap(),
            TransferPacket::Ack(7)
        ));
        assert!(decode_transfer_packet(&[0, 3, 0]).is_err());
    }

    #[test]
    fn decodes_error_packets_and_rejects_malformed_ones() {
        match decode_transfer_packet(&[0, 5, 0, 1, b'x', 0]).unwrap() {
            TransferPacket::Error { code, message } => {
                assert_eq!(code, 1);
                assert_eq!(message, "x");
            }
            _ => panic!("expected ERROR packet"),
        }
        assert!(decode_transfer_packet(&[0, 5, 0, 1]).is_err()); // missing terminator
        assert!(decode_transfer_packet(&[0, 5, 0, 1, b'x']).is_err()); // missing terminator
    }

    #[test]
    fn rejects_invalid_or_duplicate_options_and_preserves_netascii() {
        // Valid opcode + filename + mode, but missing final NUL.
        assert!(parse_request(&[0, 1, b'a', 0, b'o', b'c', b't', b'e', b't']).is_err());
        assert!(parse_request(&[
            0, 1, b'a', 0, b'o', b'c', b't', b'e', b't', 0, b't', b's', b'i', b'z', b'e', 0, b'0',
            0, b't', b's', b'i', b'z', b'e', 0, b'0', 0
        ])
        .is_err());
        let request = parse_request(&[
            0, 1, b'a', 0, b'o', b'c', b't', b'e', b't', 0, b'b', b'l', b'k', b's', b'i', b'z',
            b'e', 0, b'7', 0,
        ])
        .unwrap();
        assert!(negotiate(&request, 0, true).is_err());
        let encoded = encode_netascii(b"a\nb\rc");
        assert_eq!(encoded, b"a\r\nb\r\0c");
        let mut pending = false;
        assert_eq!(super::decode_netascii(&encoded[..4], &mut pending), b"a\nb");
        assert_eq!(super::decode_netascii(&encoded[4..], &mut pending), b"\rc");
    }

    #[test]
    fn parse_request_rejects_non_ascii_filename() {
        // Filename contains 0xff, which is not valid netascii.
        assert!(parse_request(&[0, 1, 0xff, 0, b'o', b'c', b't', b'e', b't', 0]).is_err());
    }
}
