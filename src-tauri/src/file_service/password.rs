//! File service password storage.
//!
//! The built-in FTP/TFTP/SFTP servers authenticate with a service password
//! that must be retrievable (it is a server-side shared secret, not a client
//! credential), so it lives in the OS credential vault as a plain keyring
//! entry instead of the redb settings table.

use keyring::Error;

use crate::credentials::keyring_entry;

const FILE_SERVICE_PASSWORD_ACCOUNT: &str = "file-service-password";
pub(crate) const DEFAULT_FILE_SERVICE_PASSWORD: &str = "admin";

/// Secret backend used by the file service password. Abstracted so the
/// password logic is testable without a real OS credential vault.
pub(crate) trait PasswordVault {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, password: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub(crate) struct KeyringPasswordVault;

impl PasswordVault for KeyringPasswordVault {
    fn read(&self) -> Result<Option<String>, String> {
        let entry = keyring_entry(FILE_SERVICE_PASSWORD_ACCOUNT)?;
        match entry.get_password() {
            Ok(value) if !value.is_empty() => Ok(Some(value)),
            Ok(_) => Ok(None),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("failed to read file service password: {error}")),
        }
    }

    fn write(&self, password: &str) -> Result<(), String> {
        keyring_entry(FILE_SERVICE_PASSWORD_ACCOUNT)?
            .set_password(password)
            .map_err(|error| format!("failed to store file service password: {error}"))
    }

    fn delete(&self) -> Result<(), String> {
        let entry = keyring_entry(FILE_SERVICE_PASSWORD_ACCOUNT)?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("failed to delete file service password: {error}")),
        }
    }
}

/// Resolves the effective file service password at startup: the keyring
/// entry when present, otherwise the built-in default.
pub(crate) fn resolve_password(vault: &impl PasswordVault) -> String {
    match vault.read() {
        Ok(Some(password)) => password,
        Ok(None) => DEFAULT_FILE_SERVICE_PASSWORD.to_string(),
        Err(error) => {
            log::warn!(
                target: "file_service.password",
                "file service password keyring read failed, falling back to the default: {error}"
            );
            DEFAULT_FILE_SERVICE_PASSWORD.to_string()
        }
    }
}

/// Stores a new file service password in the keyring. An empty password
/// resets the service to the built-in default and removes the keyring entry.
pub(crate) fn set_password(vault: &impl PasswordVault, password: &str) -> Result<String, String> {
    if password.is_empty() {
        vault.delete()?;
        return Ok(DEFAULT_FILE_SERVICE_PASSWORD.to_string());
    }
    vault.write(password)?;
    Ok(password.to_string())
}

pub(crate) fn is_explicit_password(password: &str) -> bool {
    password != DEFAULT_FILE_SERVICE_PASSWORD
}

/// 恒定时间比较，避免口令校验在首个不匹配字节处提前返回而泄露时序信息。
/// 长度不等时直接失败（长度本身不视为秘密）。FTP 与 SFTP 服务端共用。
/// 注：ring 0.17 的 constant_time 模块已标记 deprecated/internal（不承诺
/// 侧信道语义），依赖树中无其它审定型常量时间比较实现，故保留此前手写版。
pub(crate) fn passwords_equal(provided: &str, expected: &str) -> bool {
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    if provided.len() != expected.len() {
        return false;
    }
    provided
        .iter()
        .zip(expected.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::{
        is_explicit_password, passwords_equal, resolve_password, set_password, PasswordVault,
        DEFAULT_FILE_SERVICE_PASSWORD,
    };

    #[derive(Default)]
    struct FakeVault {
        value: Mutex<Option<String>>,
    }

    impl PasswordVault for FakeVault {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.value.lock().clone())
        }

        fn write(&self, password: &str) -> Result<(), String> {
            *self.value.lock() = Some(password.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), String> {
            *self.value.lock() = None;
            Ok(())
        }
    }

    #[test]
    fn keyring_password_wins_over_the_default() {
        let vault = FakeVault::default();
        vault.write("keyring-secret").unwrap();

        assert_eq!(resolve_password(&vault), "keyring-secret");
    }

    #[test]
    fn missing_password_falls_back_to_the_default() {
        let vault = FakeVault::default();

        assert_eq!(resolve_password(&vault), DEFAULT_FILE_SERVICE_PASSWORD);
        assert!(!is_explicit_password(DEFAULT_FILE_SERVICE_PASSWORD));
    }

    #[test]
    fn empty_password_resets_to_default_and_clears_the_vault() {
        let vault = FakeVault::default();
        vault.write("secret").unwrap();

        let resolved = set_password(&vault, "").unwrap();

        assert_eq!(resolved, DEFAULT_FILE_SERVICE_PASSWORD);
        assert_eq!(vault.read().unwrap(), None);
    }

    #[test]
    fn non_empty_password_is_written_to_the_vault() {
        let vault = FakeVault::default();

        let resolved = set_password(&vault, "s3cret").unwrap();

        assert_eq!(resolved, "s3cret");
        assert_eq!(vault.read().unwrap().as_deref(), Some("s3cret"));
        assert!(is_explicit_password("s3cret"));
    }

    #[test]
    fn password_comparison_matches_only_identical_passwords() {
        assert!(passwords_equal("s3cret", "s3cret"));
        assert!(!passwords_equal("s3cret", "s3creT"));
        assert!(!passwords_equal("s3cret", "s3cret-longer"));
        assert!(!passwords_equal("", "s3cret"));
    }
}
