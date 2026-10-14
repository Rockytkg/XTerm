use ring::rand::{SecureRandom, SystemRandom};

pub(crate) fn new_id() -> String {
    let rng = SystemRandom::new();
    let mut bytes = [0_u8; 16];
    rng.fill(&mut bytes).expect("system RNG must be available");
    uuid_from_bytes(bytes, 4)
}

fn uuid_from_bytes(mut bytes: [u8; 16], version: u8) -> String {
    bytes[6] = (bytes[6] & 0x0f) | ((version & 0x0f) << 4);
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        u64::from_be_bytes([
            0, 0, bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ])
    )
}

#[cfg(test)]
mod tests {
    use super::{new_id, uuid_from_bytes};

    #[test]
    fn uuid_from_bytes_formats_rfc4122_layout() {
        let id = uuid_from_bytes([0xab; 16], 4);
        assert_eq!(id.len(), 36);
        for (index, ch) in id.chars().enumerate() {
            match index {
                8 | 13 | 18 | 23 => assert_eq!(ch, '-', "hyphen at {index}"),
                14 => assert_eq!(ch, '4', "version nibble"),
                19 => assert!(matches!(ch, '8' | '9' | 'a' | 'b'), "variant bits"),
                _ => assert!(ch.is_ascii_hexdigit(), "hex digit at {index}"),
            }
        }
    }

    #[test]
    fn uuid_from_bytes_uses_big_endian_field_order() {
        let bytes = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba,
            0xdc, 0xfe,
        ];
        assert_eq!(
            uuid_from_bytes(bytes, 4),
            "01234567-89ab-4def-9032-547698badcfe"
        );
    }

    #[test]
    fn new_id_generates_distinct_v4_ids() {
        let first = new_id();
        let second = new_id();
        assert_ne!(first, second);
        assert_eq!(first.chars().nth(14), Some('4'));
    }
}
