use keyring::{Entry, Error};

use super::crypto::{decode_hex, encode_hex, validate_key_bytes};

const KEYRING_SERVICE: &str = "com.liushicong.xterm";
const KEYRING_ACCOUNT: &str = "credential-encryption-key";

pub(super) fn read_platform_key() -> Result<Option<Vec<u8>>, String> {
    let entry = keyring_entry(KEYRING_ACCOUNT)?;
    match entry.get_password() {
        Ok(raw) => {
            let bytes =
                decode_hex(&raw).map_err(|error| format!("credential encryption key: {error}"))?;
            validate_key_bytes(&bytes)?;
            Ok(Some(bytes))
        }
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("failed to read credential encryption key: {error}")),
    }
}

pub(super) fn write_platform_key(key: &[u8]) -> Result<(), String> {
    validate_key_bytes(key)?;
    let entry = keyring_entry(KEYRING_ACCOUNT)?;
    entry
        .set_password(&encode_hex(key))
        .map_err(|error| format!("failed to store credential encryption key: {error}"))?;
    log::debug!(
        target: "credentials.platform_key",
        "credential encryption key stored in OS credential vault"
    );
    Ok(())
}

/// Opens a keyring entry under the application service name.
/// Other modules (e.g. the file service) use this instead of touching `keyring`
/// directly so the store choice and error wording stay consistent.
pub(crate) fn keyring_entry(account: &str) -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, account)
        .map_err(|error| format!("failed to open OS credential vault: {error}"))
}
