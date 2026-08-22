use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

use super::platform_key::{read_platform_key, write_platform_key};

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const CREDENTIALS_AAD: &[u8] = b"xterm.credentials.v1";

/// Process-local cache of the credential encryption key. Reading the OS
/// credential vault on every encrypt/decrypt is expensive (and on some
/// platforms prompts or serializes through a system service), so the key
/// bytes are read once and reused. The cache is updated in place when a new
/// key is generated, so it never goes stale: the platform key is only ever
/// written by `credential_key_bytes` itself.
static KEY_CACHE: parking_lot::RwLock<Option<Vec<u8>>> = parking_lot::RwLock::new(None);

pub(super) fn encrypt_secret(value: &str) -> Result<String, String> {
    let mut plaintext = value.as_bytes().to_vec();
    let mut nonce = [0_u8; NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| "failed to generate credential encryption nonce".to_string())?;

    let key = credential_key(true)?;
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(CREDENTIALS_AAD),
        &mut plaintext,
    )
    .map_err(|_| "failed to encrypt credential secret".to_string())?;

    Ok(encode_hex(&nonce) + &encode_hex(&plaintext))
}

pub(super) fn decrypt_secret(hex: Option<&str>) -> Result<Option<String>, String> {
    let Some(hex) = hex else {
        return Ok(None);
    };
    let nonce_hex_len = NONCE_BYTES * 2;
    if hex.len() <= nonce_hex_len {
        return Err("credential secret hex is too short".to_string());
    }
    let nonce = decode_hex(&hex[..nonce_hex_len])?;
    let ciphertext = decode_hex(&hex[nonce_hex_len..])?;
    let key = credential_key(false)?;
    let plaintext = open_envelope(&key, &nonce, &ciphertext)
        .map_err(|_| "failed to decrypt credential secret".to_string())?;

    String::from_utf8(plaintext)
        .map(Some)
        .map_err(|error| format!("decrypted credential secret is not UTF-8: {error}"))
}

fn open_envelope(
    key: &LessSafeKey,
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, ring::error::Unspecified> {
    let mut buffer = ciphertext.to_vec();
    let plaintext_len = key
        .open_in_place(
            Nonce::try_assume_unique_for_key(nonce)?,
            Aad::from(CREDENTIALS_AAD),
            &mut buffer,
        )?
        .len();
    buffer.truncate(plaintext_len);
    Ok(buffer)
}

fn credential_key(create_if_missing: bool) -> Result<LessSafeKey, String> {
    let key_bytes = credential_key_bytes(create_if_missing)?;
    if key_bytes.len() != KEY_BYTES {
        return Err("credential encryption key has invalid length".to_string());
    }

    let unbound = UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
        .map_err(|_| "failed to initialize credential encryption key".to_string())?;
    Ok(LessSafeKey::new(unbound))
}

fn credential_key_bytes(create_if_missing: bool) -> Result<Vec<u8>, String> {
    if let Some(bytes) = KEY_CACHE.read().as_ref() {
        return Ok(bytes.clone());
    }

    // Load-or-generate must be serialized under the write lock: two
    // concurrent first-run callers would otherwise generate different keys
    // and overwrite each other's keyring entry, permanently orphaning any
    // secret encrypted with the losing key in between.
    let mut cache = KEY_CACHE.write();
    if let Some(bytes) = cache.as_ref() {
        return Ok(bytes.clone());
    }

    let bytes = match read_platform_key()? {
        Some(bytes) => bytes,
        None => {
            if !create_if_missing {
                return Err(
                    "credential encryption key was not found in platform storage".to_string(),
                );
            }
            let mut fresh = [0_u8; KEY_BYTES];
            SystemRandom::new()
                .fill(&mut fresh)
                .map_err(|_| "failed to generate credential encryption key".to_string())?;
            write_platform_key(&fresh)?;
            fresh.to_vec()
        }
    };
    *cache = Some(bytes.clone());
    Ok(bytes)
}

pub(super) fn validate_key_bytes(key: &[u8]) -> Result<(), String> {
    if key.len() != KEY_BYTES {
        return Err("credential encryption key has invalid length".to_string());
    }
    Ok(())
}

pub(super) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(super) fn decode_hex(raw: &str) -> Result<Vec<u8>, String> {
    if !raw.len().is_multiple_of(2) {
        return Err("hex value has odd length".to_string());
    }

    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0])?;
            let low = hex_value(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("hex value contains invalid characters".to_string()),
    }
}
