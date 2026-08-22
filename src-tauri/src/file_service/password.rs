//! File service password storage.
//!
//! The built-in FTP/TFTP/SFTP servers authenticate with a service password
//! that must be retrievable (it is a server-side shared secret, not a client
//! credential), so it lives in the OS credential vault as a plain keyring
//! entry instead of the redb settings table. Older releases stored it in
//! redb under `fileServicePassword`; that value is migrated into the keyring
//! on startup and only kept when the keyring is unavailable, so a machine
//! without a working credential vault never locks the user out.

use keyring::Error;

use crate::{credentials::keyring_entry, storage::repository::SettingsRepository};

use super::models::FILE_SERVICE_PASSWORD_KEY;

const FILE_SERVICE_PASSWORD_ACCOUNT: &str = "file-service-password";
pub(crate) const DEFAULT_FILE_SERVICE_PASSWORD: &str = "admin";

/// Secret backend used by the file service password. Abstracted so the
/// migration logic is testable without a real OS credential vault.
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

/// Resolves the effective file service password at startup.
///
/// Priority: keyring entry, then the legacy redb setting (migrated into the
/// keyring when possible), then the built-in default. The returned flag is
/// `true` when a password is explicitly configured (differs from the
/// default), which is what the public config DTO reports as `passwordSet`.
pub(crate) fn resolve_password(
    store: &impl SettingsRepository,
    vault: &impl PasswordVault,
) -> String {
    match vault.read() {
        Ok(Some(password)) => {
            // The keyring is authoritative now; drop any legacy redb copy.
            clear_legacy_setting(store);
            return password;
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                target: "file_service.password",
                "file service password keyring read failed, falling back: {error}"
            );
        }
    }

    let legacy = store
        .setting_value(FILE_SERVICE_PASSWORD_KEY)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());
    if let Some(legacy) = legacy {
        match vault.write(&legacy) {
            Ok(()) => {
                if let Err(error) = store.delete_setting(FILE_SERVICE_PASSWORD_KEY) {
                    log::warn!(
                        target: "file_service.password",
                        "file service password migrated to keyring but the legacy \
                         redb value could not be removed: {error}"
                    );
                }
            }
            Err(error) => {
                // Keep the redb value so the next start can retry the
                // migration instead of losing the configured password.
                log::warn!(
                    target: "file_service.password",
                    "file service password migration to keyring failed, keeping \
                     the legacy redb value: {error}"
                );
            }
        }
        return legacy;
    }

    DEFAULT_FILE_SERVICE_PASSWORD.to_string()
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

fn clear_legacy_setting(store: &impl SettingsRepository) {
    match store.setting_value(FILE_SERVICE_PASSWORD_KEY) {
        Ok(Some(_)) => {
            if let Err(error) = store.delete_setting(FILE_SERVICE_PASSWORD_KEY) {
                log::warn!(
                    target: "file_service.password",
                    "failed to remove legacy redb file service password: {error}"
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                target: "file_service.password",
                "failed to check legacy redb file service password: {error}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use parking_lot::Mutex;

    use super::{
        is_explicit_password, resolve_password, set_password, PasswordVault,
        DEFAULT_FILE_SERVICE_PASSWORD,
    };
    use crate::{
        file_service::models::FILE_SERVICE_PASSWORD_KEY, storage::repository::SettingsRepository,
    };

    #[derive(Default)]
    struct FakeStore {
        values: Mutex<HashMap<String, String>>,
        fail_delete: bool,
    }

    impl SettingsRepository for FakeStore {
        fn log_level(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn set_log_level(&self, _level: &str) -> Result<(), String> {
            Ok(())
        }

        fn preferences(&self) -> Result<crate::storage::AppPreferences, String> {
            Ok(crate::storage::AppPreferences::default())
        }

        fn set_preferences(
            &self,
            _preferences: &crate::storage::AppPreferences,
        ) -> Result<(), String> {
            Ok(())
        }

        fn setting_value(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.values.lock().get(key).cloned())
        }

        fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
            self.values
                .lock()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete_setting(&self, key: &str) -> Result<(), String> {
            if self.fail_delete {
                return Err("simulated delete failure".to_string());
            }
            self.values.lock().remove(key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeVault {
        value: Mutex<Option<String>>,
        fail_writes: bool,
    }

    impl PasswordVault for FakeVault {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.value.lock().clone())
        }

        fn write(&self, password: &str) -> Result<(), String> {
            if self.fail_writes {
                return Err("simulated keyring failure".to_string());
            }
            *self.value.lock() = Some(password.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), String> {
            *self.value.lock() = None;
            Ok(())
        }
    }

    #[test]
    fn keyring_password_wins_and_clears_legacy_setting() {
        let store = FakeStore::default();
        store
            .set_setting(FILE_SERVICE_PASSWORD_KEY, "legacy")
            .unwrap();
        let vault = FakeVault::default();
        vault.write("keyring-secret").unwrap();

        let password = resolve_password(&store, &vault);

        assert_eq!(password, "keyring-secret");
        assert_eq!(
            store.setting_value(FILE_SERVICE_PASSWORD_KEY).unwrap(),
            None
        );
    }

    #[test]
    fn legacy_redb_password_is_migrated_into_the_keyring() {
        let store = FakeStore::default();
        store
            .set_setting(FILE_SERVICE_PASSWORD_KEY, "legacy")
            .unwrap();
        let vault = FakeVault::default();

        let password = resolve_password(&store, &vault);

        assert_eq!(password, "legacy");
        assert_eq!(vault.read().unwrap().as_deref(), Some("legacy"));
        assert_eq!(
            store.setting_value(FILE_SERVICE_PASSWORD_KEY).unwrap(),
            None
        );
    }

    #[test]
    fn failed_migration_keeps_the_legacy_value_for_a_later_retry() {
        let store = FakeStore::default();
        store
            .set_setting(FILE_SERVICE_PASSWORD_KEY, "legacy")
            .unwrap();
        let vault = FakeVault {
            fail_writes: true,
            ..Default::default()
        };

        let password = resolve_password(&store, &vault);

        assert_eq!(password, "legacy");
        assert_eq!(
            store.setting_value(FILE_SERVICE_PASSWORD_KEY).unwrap(),
            Some("legacy".to_string())
        );
    }

    #[test]
    fn missing_password_falls_back_to_the_default() {
        let store = FakeStore::default();
        let vault = FakeVault::default();

        assert_eq!(
            resolve_password(&store, &vault),
            DEFAULT_FILE_SERVICE_PASSWORD
        );
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
}
