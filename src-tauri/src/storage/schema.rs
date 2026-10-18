use redb::{Database, ReadableTable, TableDefinition};
use serde::{de::DeserializeOwned, Serialize};

use super::settings::initial_setting_entries;

pub(super) const SETTINGS: TableDefinition<&str, &str> = TableDefinition::new("settings");
pub(super) const CREDENTIALS: TableDefinition<&str, &[u8]> = TableDefinition::new("credentials");
pub(super) const CONNECTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("connections");
pub(super) const HOST_KEYS: TableDefinition<&str, &[u8]> = TableDefinition::new("ssh_host_keys");

pub(super) fn initialize_schema(database: &Database) -> Result<(), String> {
    let write_txn = database
        .begin_write()
        .map_err(|error| format!("failed to initialize Redb write transaction: {error}"))?;
    {
        write_txn
            .open_table(CREDENTIALS)
            .map_err(|error| format!("failed to initialize credentials table: {error}"))?;
        write_txn
            .open_table(CONNECTIONS)
            .map_err(|error| format!("failed to initialize connections table: {error}"))?;
        write_txn
            .open_table(HOST_KEYS)
            .map_err(|error| format!("failed to initialize SSH host keys table: {error}"))?;
        let mut settings = write_txn
            .open_table(SETTINGS)
            .map_err(|error| format!("failed to initialize settings table: {error}"))?;
        for entry in initial_setting_entries() {
            if settings
                .get(entry.key)
                .map_err(|error| format!("failed to read setting '{}': {error}", entry.key))?
                .is_none()
            {
                settings
                    .insert(entry.key, entry.value.as_str())
                    .map_err(|error| format!("failed to seed setting '{}': {error}", entry.key))?;
            }
        }
    }
    write_txn
        .commit()
        .map_err(|error| format!("failed to commit Redb schema initialization: {error}"))
}

pub(super) fn encode_record<T: Serialize>(label: &str, value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| format!("failed to encode {label}: {error}"))
}

pub(super) fn decode_record<T: DeserializeOwned>(label: &str, bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("failed to decode {label}: {error}"))
}
