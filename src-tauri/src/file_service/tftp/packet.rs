//! Pure TFTP wire codec: request parsing, option negotiation (RFC 2347/2348/
//! 2349/7440), packet encoding/decoding and netascii conversion.  No socket
//! or filesystem access happens here, so everything is unit-testable.

use std::time::Duration;

use super::error::TransferError;

pub(super) const TFTP_DEFAULT_BLOCK_SIZE: usize = 512;
pub(super) const TFTP_MAX_BLOCK_SIZE: usize = 1468;
pub(super) const TFTP_MAX_WINDOW_SIZE: usize = 64;
/// Receive buffer: header + largest negotiated block + slack for options.
/// Also bounds RRQ/WRQ packets, so requests are not size-checked separately.
pub(super) const MAX_PACKET_SIZE: usize = 4 + TFTP_MAX_BLOCK_SIZE + 1024;

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
    // Parsing is deliberately liberal (Postel's law): boot-ROM TFTP clients
    // routinely emit requests a strict RFC 2347 parser rejects — U-Boot built
    // with CONFIG_MCAST_TFTP appends a `multicast` option with an EMPTY value
    // (`multicast\0\0`), other firmware pads the packet with NULs or junk to
    // a fixed buffer size, or forgets the final NUL.  Strict servers answer
    // these with ERROR 4 ("malformed request"), which is exactly where
    // lenient servers like tftpd32 succeed instead.  Parse the leading
    // NUL-separated fields, allow empty option values, stop at the first
    // empty option name, and silently ignore any trailing bytes.
    let mut fields = packet[2..].split(|byte| *byte == 0);
    let filename = ascii_field(fields.next().unwrap_or_default(), "filename")?;
    let mode = match ascii_field(fields.next().unwrap_or_default(), "mode")?
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
    while let Some(name) = fields.next() {
        if name.is_empty() {
            break; // NUL padding terminates the option list
        }
        let Some(value) = fields.next() else { break }; // dangling name: ignore
        if !name.iter().all(u8::is_ascii) || !value.iter().all(u8::is_ascii) {
            continue; // junk tail, not an option pair
        }
        let key = String::from_utf8_lossy(name).to_ascii_lowercase();
        if options.iter().any(|(existing, _)| existing == &key) {
            continue; // duplicate option: first one wins
        }
        options.push((key, String::from_utf8_lossy(value).into_owned()));
    }
    Ok(Request {
        opcode,
        filename,
        mode,
        options,
    })
}

/// Builds the OACK answer for the requested RFC 2347 options.  Negotiation
/// is deliberately lenient (same Postel rationale as `parse_request`): an
/// option whose value is empty, unparseable or out of range is simply not
/// acknowledged, so the transfer falls back to RFC 1350 defaults instead of
/// failing the request outright.  Unknown options are likewise ignored (not
/// acknowledged), per RFC 2347 §4.
pub(super) fn negotiate(request: &Request, transfer_size: u64, read: bool) -> Negotiated {
    let mut block_size = TFTP_DEFAULT_BLOCK_SIZE;
    let mut timeout_seconds = 3u64;
    let mut window_size = 1usize;
    let mut oack = Vec::new();
    for (key, value) in &request.options {
        match key.as_str() {
            "blksize" => {
                if let Ok(value) = value.parse::<usize>() {
                    if (8..=65464).contains(&value) {
                        // The server may acknowledge a smaller value than requested.
                        block_size = value.min(TFTP_MAX_BLOCK_SIZE);
                        oack.push(("blksize".to_string(), block_size.to_string()));
                    }
                }
            }
            "timeout" => {
                if let Ok(value) = value.parse::<u64>() {
                    if (1..=255).contains(&value) {
                        timeout_seconds = value;
                        oack.push(("timeout".to_string(), timeout_seconds.to_string()));
                    }
                }
            }
            "tsize" => {
                // RFC 2349 reports the octets the client will receive.  On
                // RRQ the requested value (should be 0) is irrelevant; some
                // clients send it empty or garbage — answer the size anyway.
                if read {
                    oack.push(("tsize".to_string(), transfer_size.to_string()));
                } else if let Ok(value) = value.parse::<u64>() {
                    oack.push(("tsize".to_string(), value.to_string()));
                }
            }
            "windowsize" => {
                if let Ok(value) = value.parse::<usize>() {
                    if (1..=65535).contains(&value) {
                        window_size = value.min(TFTP_MAX_WINDOW_SIZE);
                        oack.push(("windowsize".to_string(), window_size.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    Negotiated {
        block_size,
        window_size,
        timeout: Duration::from_secs(timeout_seconds),
        oack,
    }
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
        // Boot-ROM clients may pad the fixed four-byte ACK to an aligned UDP
        // payload; trailing bytes carry no meaning, so they are ignored.
        4 => Ok(TransferPacket::Ack(be16(packet, 2))),
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

/// Converts received netascii back to the local representation, appending to
/// `output` (a buffer reused across blocks, so the receive path performs no
/// per-block allocation).  `pending_cr` carries a CR split across two DATA
/// blocks between calls.
pub(super) fn decode_netascii_into(input: &[u8], pending_cr: &mut bool, output: &mut Vec<u8>) {
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
}

#[cfg(test)]
mod tests {
    use super::{
        decode_transfer_packet, encode_netascii, negotiate, parse_request, Opcode, TransferPacket,
        TFTP_MAX_BLOCK_SIZE,
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
        let options = negotiate(&request, 123, false);
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
        let options = negotiate(&request, 0, false);
        assert_eq!(options.window_size, 8);
        assert!(options
            .oack
            .iter()
            .any(|option| option == &("tsize".into(), "42".into())));
        assert_eq!(super::oack_packet(&options.oack)[..2], [0, 6]);
    }

    #[test]
    fn packet_codec_validates_rfc_packet_headers() {
        let packet = [0, 3, 0, 7, b'a', b'b', b'c'];
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
        // Trailing bytes on an ACK (boot-ROM padding) are ignored.
        assert!(matches!(
            decode_transfer_packet(&[0, 4, 0, 7, 1]).unwrap(),
            TransferPacket::Ack(7)
        ));
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

    /// Builds the RRQ bytes `00 01 <fields...>` with each field NUL-terminated.
    fn rrq(fields: &[&[u8]]) -> Vec<u8> {
        let mut packet = vec![0, 1];
        for field in fields {
            packet.extend_from_slice(field);
            packet.push(0);
        }
        packet
    }

    #[test]
    fn tolerates_uboot_mcast_empty_option_value() {
        // Regression: U-Boot built with CONFIG_MCAST_TFTP (Marvell switch
        // boot ROMs, e.g. the egiga driver) appends a `multicast` option with
        // an EMPTY value (`multicast\0\0`).  Rejecting the empty field made
        // the server answer ERROR 4 ("malformed request") — the exact failure
        // those devices show against strict servers while tftpd32 works.
        let request = parse_request(&rrq(&[
            b"nos.img",
            b"octet",
            b"timeout",
            b"5",
            b"blksize",
            b"1468",
            b"multicast",
            b"",
        ]))
        .unwrap();
        assert_eq!(request.filename, "nos.img");
        assert_eq!(request.options.len(), 3);
        let negotiated = negotiate(&request, 14601478, true);
        assert_eq!(negotiated.block_size, TFTP_MAX_BLOCK_SIZE);
        assert!(negotiated
            .oack
            .iter()
            .any(|option| option == &("timeout".into(), "5".into())));
        // The empty-value option is not acknowledged.
        assert!(!negotiated.oack.iter().any(|(key, _)| key == "multicast"));
    }

    #[test]
    fn tolerates_padded_and_unterminated_requests() {
        // NUL padding to a fixed buffer size.
        let mut padded = rrq(&[b"nos.img", b"octet", b"blksize", b"1468"]);
        padded.extend_from_slice(&[0; 200]);
        let request = parse_request(&padded).unwrap();
        assert_eq!(request.filename, "nos.img");
        assert_eq!(request.options.len(), 1);

        // A missing final NUL still parses.
        let mut unterminated = rrq(&[b"nos.img", b"octet", b"blksize", b"1468"]);
        unterminated.pop();
        assert_eq!(parse_request(&unterminated).unwrap().options.len(), 1);

        // Trailing garbage after the last field is ignored.
        let mut junk = rrq(&[b"nos.img", b"octet"]);
        junk.extend_from_slice(&[0xff, 0xfe, 0xfd]);
        assert_eq!(parse_request(&junk).unwrap().filename, "nos.img");

        // Duplicate options keep the first value.
        let request = parse_request(&rrq(&[
            b"nos.img", b"octet", b"blksize", b"1024", b"blksize", b"512",
        ]))
        .unwrap();
        assert_eq!(
            request.options,
            [("blksize".to_string(), "1024".to_string())]
        );
    }

    #[test]
    fn invalid_option_values_fall_back_to_defaults_instead_of_failing() {
        let request = parse_request(&rrq(&[
            b"nos.img",
            b"octet",
            b"blksize",
            b"7", // below the RFC 2348 minimum of 8
            b"timeout",
            b"0", // below the RFC 2349 minimum of 1
            b"windowsize",
            b"99999999", // above the RFC 7440 maximum of 65535
            b"tsize",
            b"not-a-size", // unparseable
        ]))
        .unwrap();
        let negotiated = negotiate(&request, 0, false);
        assert_eq!(negotiated.block_size, super::TFTP_DEFAULT_BLOCK_SIZE);
        assert_eq!(negotiated.window_size, 1);
        assert!(negotiated.oack.is_empty());
    }

    #[test]
    fn rrq_tsize_is_answered_even_when_the_value_is_not_zero() {
        // Strictly "RRQ tsize must be zero" is RFC 2349, but real clients
        // send other values; answer the actual size instead of failing.
        let request = parse_request(&rrq(&[b"nos.img", b"octet", b"tsize", b"99"])).unwrap();
        let negotiated = negotiate(&request, 1234, true);
        assert!(negotiated
            .oack
            .iter()
            .any(|option| option == &("tsize".into(), "1234".into())));
    }

    #[test]
    fn still_rejects_unservable_requests() {
        assert!(parse_request(&[0, 1]).is_err()); // too short for filename + mode
        assert!(parse_request(&[0, 1, 0, 0]).is_err()); // empty filename
        assert!(parse_request(&rrq(&[b"nos.img"])).is_err()); // missing mode
        assert!(parse_request(&rrq(&[b"nos.img", b"mail"])).is_err()); // obsolete mode
        assert!(parse_request(&[0, 9, b'a', 0, b'o', b'c', b't', b'e', b't', 0]).is_err());
        // Filename contains 0xff, which is not valid netascii.
        assert!(parse_request(&[0, 1, 0xff, 0, b'o', b'c', b't', b'e', b't', 0]).is_err());
    }

    #[test]
    fn preserves_netascii_conversion() {
        let encoded = encode_netascii(b"a\nb\rc");
        assert_eq!(encoded, b"a\r\nb\r\0c");
        let mut pending = false;
        let mut decoded = Vec::new();
        super::decode_netascii_into(&encoded[..4], &mut pending, &mut decoded);
        assert_eq!(decoded, b"a\nb");
        decoded.clear();
        super::decode_netascii_into(&encoded[4..], &mut pending, &mut decoded);
        assert_eq!(decoded, b"\rc");
    }
}
