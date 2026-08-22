use super::*;
use redb::ReadableDatabase;

use super::schema::HOST_KEYS;

fn host_key_connection_key(connection_id: &str) -> String {
    connection_id.trim().to_string()
}

impl Store {
    pub fn host_key(&self, connection_id: &str) -> Result<Option<String>, String> {
        let key = host_key_connection_key(connection_id);
        if key.is_empty() {
            return Ok(None);
        }
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start SSH host key read transaction: {error}"))?;
        let table = read_txn
            .open_table(HOST_KEYS)
            .map_err(|error| format!("failed to open SSH host keys table: {error}"))?;
        table
            .get(key.as_str())
            .map_err(|error| format!("failed to read SSH host key: {error}"))?
            .map(|value| {
                String::from_utf8(value.value().to_vec())
                    .map_err(|error| format!("failed to decode SSH host key: {error}"))
            })
            .transpose()
    }

    pub fn upsert_host_key(&self, connection_id: &str, fingerprint: &str) -> Result<(), String> {
        let key = host_key_connection_key(connection_id);
        if key.is_empty() {
            return Err("SSH host key connection id is required".to_string());
        }
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start SSH host key write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(HOST_KEYS)
                .map_err(|error| format!("failed to open SSH host keys table: {error}"))?;
            table
                .insert(key.as_str(), fingerprint.as_bytes())
                .map_err(|error| format!("failed to save SSH host key: {error}"))?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit SSH host key: {error}"))
    }

    pub fn delete_host_key(&self, connection_id: &str) -> Result<bool, String> {
        let key = host_key_connection_key(connection_id);
        if key.is_empty() {
            return Ok(false);
        }
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start SSH host key delete transaction: {error}"))?;
        let deleted = {
            let mut table = write_txn
                .open_table(HOST_KEYS)
                .map_err(|error| format!("failed to open SSH host keys table: {error}"))?;
            let removed = table
                .remove(key.as_str())
                .map_err(|error| format!("failed to delete SSH host key: {error}"))?;
            removed.is_some()
        };
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit SSH host key delete: {error}"))?;
        Ok(deleted)
    }
}
