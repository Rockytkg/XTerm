use redb::{Database, ReadableTable, TableDefinition};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
    migrate_credential_usernames_to_connections(&write_txn)?;
    write_txn
        .commit()
        .map_err(|error| format!("failed to commit Redb schema initialization: {error}"))
}

fn migrate_credential_usernames_to_connections(
    write_txn: &redb::WriteTransaction,
) -> Result<(), String> {
    let mut credential_usernames = HashMap::new();
    {
        let mut credentials = write_txn
            .open_table(CREDENTIALS)
            .map_err(|error| format!("failed to open credentials table for migration: {error}"))?;
        let mut changed = Vec::new();
        for row in credentials
            .iter()
            .map_err(|error| format!("failed to iterate credentials for migration: {error}"))?
        {
            let (key, value) =
                row.map_err(|error| format!("failed to read credential row: {error}"))?;
            let id = key.value().to_string();
            let mut raw = serde_json::from_slice::<Value>(value.value())
                .map_err(|error| format!("failed to decode credential '{id}': {error}"))?;
            if let Some(username) = take_string_field(&mut raw, "username") {
                if !username.trim().is_empty() {
                    credential_usernames.insert(id.clone(), username.trim().to_string());
                }
                changed.push((id, raw));
            }
        }
        for (id, raw) in changed {
            let bytes = serde_json::to_vec(&raw)
                .map_err(|error| format!("failed to encode migrated credential '{id}': {error}"))?;
            credentials
                .insert(id.as_str(), bytes.as_slice())
                .map_err(|error| format!("failed to save migrated credential '{id}': {error}"))?;
        }
    }

    if credential_usernames.is_empty() {
        return Ok(());
    }

    let mut connections = write_txn
        .open_table(CONNECTIONS)
        .map_err(|error| format!("failed to open connections table for migration: {error}"))?;
    let mut changed = Vec::new();
    for row in connections
        .iter()
        .map_err(|error| format!("failed to iterate connections for migration: {error}"))?
    {
        let (key, value) =
            row.map_err(|error| format!("failed to read connection row: {error}"))?;
        let id = key.value().to_string();
        let mut raw = serde_json::from_slice::<Value>(value.value())
            .map_err(|error| format!("failed to decode connection '{id}': {error}"))?;
        if migrate_connection_username_fields(&mut raw, &credential_usernames)? {
            changed.push((id, raw));
        }
    }
    for (id, raw) in changed {
        let bytes = serde_json::to_vec(&raw)
            .map_err(|error| format!("failed to encode migrated connection '{id}': {error}"))?;
        connections
            .insert(id.as_str(), bytes.as_slice())
            .map_err(|error| format!("failed to save migrated connection '{id}': {error}"))?;
    }
    Ok(())
}

fn migrate_connection_username_fields(
    raw: &mut Value,
    credential_usernames: &HashMap<String, String>,
) -> Result<bool, String> {
    let Some(connection) = raw.get_mut("connection").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let mut changed = false;

    let saved_credential_id = connection
        .get("saved_credential_id")
        .and_then(Value::as_str)
        .or_else(|| {
            connection
                .get("details")
                .and_then(|details| details.get("ssh"))
                .and_then(|ssh| ssh.get("savedCredentialId"))
                .and_then(Value::as_str)
        });

    if object_string_field_is_empty(connection, "user") {
        if let Some(username) =
            saved_credential_id.and_then(|id| credential_usernames.get(id.trim()))
        {
            connection.insert("user".to_string(), Value::String(username.clone()));
            changed = true;
        }
    }

    let (jump_hosts_container, jump_hosts_key) = if connection.contains_key("details") {
        let details = connection
            .get_mut("details")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "connection details is not an object".to_string())?;
        let ssh = details
            .get_mut("ssh")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "connection details.ssh is not an object".to_string())?;
        (ssh, "jumpHosts")
    } else {
        (connection, "jump_hosts")
    };

    let Some(jump_hosts) = jump_hosts_container.get_mut(jump_hosts_key) else {
        return Ok(changed);
    };
    let Some(raw_jump_hosts) = jump_hosts.as_str() else {
        return Ok(changed);
    };
    let Ok(mut hops) = serde_json::from_str::<Value>(raw_jump_hosts) else {
        return Ok(changed);
    };
    let Some(hops_array) = hops.as_array_mut() else {
        return Ok(changed);
    };
    let mut hops_changed = false;
    for hop in hops_array {
        let Some(hop) = hop.as_object_mut() else {
            continue;
        };
        if !object_string_field_is_empty(hop, "user") {
            continue;
        }
        if let Some(username) = hop
            .get("savedCredentialId")
            .and_then(Value::as_str)
            .and_then(|id| credential_usernames.get(id.trim()))
        {
            hop.insert("user".to_string(), Value::String(username.clone()));
            hops_changed = true;
        }
    }
    if hops_changed {
        let serialized = serde_json::to_string(&hops)
            .map_err(|error| format!("failed to encode migrated jump hosts: {error}"))?;
        *jump_hosts = Value::String(serialized);
        changed = true;
    }
    Ok(changed)
}

fn take_string_field(raw: &mut Value, field: &str) -> Option<String> {
    raw.as_object_mut()?
        .remove(field)
        .and_then(|value| value.as_str().map(str::to_string))
}

fn object_string_field_is_empty(map: &serde_json::Map<String, Value>, field: &str) -> bool {
    map.get(field)
        .and_then(Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
}

pub(super) fn encode_record<T: Serialize>(label: &str, value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| format!("failed to encode {label}: {error}"))
}

pub(super) fn decode_record<T: DeserializeOwned>(label: &str, bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("failed to decode {label}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::migrate_connection_username_fields;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn migrates_credential_usernames_to_connection_and_manual_jump_hosts() {
        let mut usernames = HashMap::new();
        usernames.insert("cred-main".to_string(), "admin".to_string());
        usernames.insert("cred-hop".to_string(), "jump".to_string());

        let mut flat_raw = json!({
            "position": 0,
            "connection": {
                "protocol": "ssh",
                "name": "core",
                "host": "10.0.0.1",
                "user": "",
                "saved_credential_id": "cred-main",
                "jump_hosts": "[{\"host\":\"bastion\",\"user\":\"\",\"savedCredentialId\":\"cred-hop\"}]"
            }
        });

        let changed = migrate_connection_username_fields(&mut flat_raw, &usernames)
            .expect("migration should succeed");

        assert!(changed);
        assert_eq!(flat_raw["connection"]["user"], "admin");
        let hops: serde_json::Value =
            serde_json::from_str(flat_raw["connection"]["jump_hosts"].as_str().unwrap()).unwrap();
        assert_eq!(hops[0]["user"], "jump");

        let mut nested_raw = json!({
            "position": 0,
            "connection": {
                "protocol": "ssh",
                "name": "core",
                "host": "10.0.0.1",
                "user": "",
                "options": {
                    "terminalType": "xterm-256color"
                },
                "details": {
                    "protocol": "ssh",
                    "ssh": {
                        "savedCredentialId": "cred-main",
                        "jumpHosts": "[{\"host\":\"bastion\",\"user\":\"\",\"savedCredentialId\":\"cred-hop\"}]"
                    }
                }
            }
        });

        let changed = migrate_connection_username_fields(&mut nested_raw, &usernames)
            .expect("migration should succeed");

        assert!(changed);
        assert_eq!(nested_raw["connection"]["user"], "admin");
        let hops: serde_json::Value = serde_json::from_str(
            nested_raw["connection"]["details"]["ssh"]["jumpHosts"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hops[0]["user"], "jump");
    }
}
