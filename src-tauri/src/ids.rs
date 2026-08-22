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
