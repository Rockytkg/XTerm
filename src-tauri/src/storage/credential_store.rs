use super::*;
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::collections::HashSet;

use super::models::{ConnectionDetails, StoredConnection};
use super::schema::{decode_record, encode_record, CONNECTIONS, CREDENTIALS};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredJumpHostCredentialRef {
    #[serde(default)]
    saved_credential_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredCredentialUsage {
    pub credential_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub relation: String,
}

pub(super) fn connection_saved_credential_id(connection: &StoredConnection) -> Option<String> {
    match &connection.details {
        ConnectionDetails::Ssh(details) => details.saved_credential_id.clone(),
        ConnectionDetails::Telnet(details) => details.saved_credential_id.clone(),
        ConnectionDetails::Serial(details) => details.saved_credential_id.clone(),
    }
}

pub(super) fn set_connection_saved_credential_id(
    connection: &mut StoredConnection,
    credential_id: Option<String>,
) {
    match &mut connection.details {
        ConnectionDetails::Ssh(details) => details.saved_credential_id = credential_id,
        ConnectionDetails::Telnet(details) => details.saved_credential_id = credential_id,
        ConnectionDetails::Serial(details) => details.saved_credential_id = credential_id,
    }
}

pub(super) fn connection_jump_hosts(connection: &StoredConnection) -> Option<String> {
    match &connection.details {
        ConnectionDetails::Ssh(details) => details.jump_hosts.clone(),
        _ => None,
    }
}

pub(super) fn set_connection_jump_hosts(
    connection: &mut StoredConnection,
    jump_hosts: Option<String>,
) {
    if let ConnectionDetails::Ssh(details) = &mut connection.details {
        details.jump_hosts = jump_hosts;
    }
}

fn decode_credential(id: &str, bytes: &[u8]) -> Result<StoredCredential, String> {
    decode_record(&format!("credential '{id}'"), bytes)
}

/// Highest used position + 1 within an open credentials table.
fn next_credential_position(table: &redb::Table<'_, &str, &[u8]>) -> Result<i64, String> {
    let mut next_position = 0;
    for row in table
        .iter()
        .map_err(|error| format!("failed to iterate credentials: {error}"))?
    {
        let (key, value) =
            row.map_err(|error| format!("failed to read credential row: {error}"))?;
        let credential = decode_credential(key.value(), value.value())?;
        next_position = next_position.max(credential.position + 1);
    }
    Ok(next_position)
}

impl Store {
    pub fn credentials(&self) -> Result<Vec<StoredCredentialRecord>, String> {
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start credential read transaction: {error}"))?;
        let table = read_txn
            .open_table(CREDENTIALS)
            .map_err(|error| format!("failed to open credentials table: {error}"))?;
        let mut credentials = Vec::new();
        for row in table
            .iter()
            .map_err(|error| format!("failed to iterate credentials: {error}"))?
        {
            let (key, value) =
                row.map_err(|error| format!("failed to read credential row: {error}"))?;
            credentials.push(StoredCredentialRecord {
                id: key.value().to_string(),
                credential: decode_credential(key.value(), value.value())?,
            });
        }
        credentials.sort_by(|a, b| {
            a.credential
                .position
                .cmp(&b.credential.position)
                .then_with(|| {
                    a.credential
                        .name
                        .to_ascii_lowercase()
                        .cmp(&b.credential.name.to_ascii_lowercase())
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(credentials)
    }

    pub fn reorder_credentials(&mut self, ordered_ids: &[String]) -> Result<(), String> {
        let existing_ids = self
            .credentials()?
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        let existing = existing_ids.iter().cloned().collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut reordered = Vec::with_capacity(existing_ids.len());

        for id in ordered_ids {
            if !existing.contains(id) {
                return Err(format!("credential '{id}' not found"));
            }
            if !seen.insert(id.clone()) {
                return Err(format!("credential '{id}' appears more than once"));
            }
            reordered.push(id.clone());
        }

        for id in existing_ids {
            if seen.insert(id.clone()) {
                reordered.push(id);
            }
        }

        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start credential reorder transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(CREDENTIALS)
                .map_err(|error| format!("failed to open credentials table: {error}"))?;
            for (index, id) in reordered.iter().enumerate() {
                let Some(mut credential) = table
                    .get(id.as_str())
                    .map_err(|error| format!("failed to read credential '{id}': {error}"))?
                    .map(|current| decode_credential(id, current.value()))
                    .transpose()?
                else {
                    return Err(format!("credential '{id}' not found"));
                };
                credential.position = index as i64;
                let bytes = encode_record(&format!("credential '{id}'"), &credential)?;
                table
                    .insert(id.as_str(), bytes.as_slice())
                    .map_err(|error| format!("failed to reorder credential '{id}': {error}"))?;
            }
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit credential reorder: {error}"))
    }

    /// Inserts a new credential, appending it at the end of the list.
    /// Re-saving an identical credential (same content-derived id) keeps its
    /// current position.
    pub fn insert_credential(&self, id: &str, c: &StoredCredential) -> Result<(), String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start credential write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(CREDENTIALS)
                .map_err(|error| format!("failed to open credentials table: {error}"))?;
            let existing_position = table
                .get(id)
                .map_err(|error| format!("failed to read credential '{id}': {error}"))?
                .map(|current| decode_credential(id, current.value()))
                .transpose()?
                .map(|credential| credential.position);
            let mut credential = c.clone();
            credential.position = match existing_position {
                Some(position) => position,
                None => next_credential_position(&table)?,
            };
            let bytes = encode_record(&format!("credential '{id}'"), &credential)?;
            table
                .insert(id, bytes.as_slice())
                .map_err(|error| format!("failed to save credential '{id}': {error}"))?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit credential '{id}': {error}"))
    }

    /// Atomically replaces the credential stored under `previous_id` with the
    /// given record. When the content-derived id changed, connection and jump
    /// host references are rewritten and the old record removed inside the
    /// same transaction, so a failure cannot leave duplicate or dangling
    /// credentials behind.
    pub fn update_credential(
        &self,
        previous_id: &str,
        id: &str,
        c: &StoredCredential,
    ) -> Result<(), String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start credential write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(CREDENTIALS)
                .map_err(|error| format!("failed to open credentials table: {error}"))?;
            let existing_position = table
                .get(previous_id)
                .map_err(|error| format!("failed to read credential '{previous_id}': {error}"))?
                .map(|current| decode_credential(previous_id, current.value()))
                .transpose()?
                .map(|credential| credential.position);
            let mut credential = c.clone();
            credential.position = match existing_position {
                Some(position) => position,
                None => next_credential_position(&table)?,
            };
            let bytes = encode_record(&format!("credential '{id}'"), &credential)?;
            table
                .insert(id, bytes.as_slice())
                .map_err(|error| format!("failed to save credential '{id}': {error}"))?;
            if previous_id != id {
                table.remove(previous_id).map_err(|error| {
                    format!("failed to delete renamed credential '{previous_id}': {error}")
                })?;
            }
        }
        if previous_id != id {
            rewrite_credential_references(&write_txn, previous_id, id)?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit credential '{id}': {error}"))
    }

    pub fn credential_by_id(&self, id: &str) -> Result<Option<StoredCredential>, String> {
        let read_txn = self.database.begin_read().map_err(|error| {
            format!("failed to start credential by id read transaction: {error}")
        })?;
        let table = read_txn
            .open_table(CREDENTIALS)
            .map_err(|error| format!("failed to open credentials table: {error}"))?;
        table
            .get(id)
            .map_err(|error| format!("failed to read credential '{id}': {error}"))?
            .map(|value| decode_credential(id, value.value()))
            .transpose()
    }
}

/// Rewrites saved-credential references on every connection (and SSH jump
/// host) from `old_id` to `new_id` inside an already-open write transaction.
fn rewrite_credential_references(
    write_txn: &redb::WriteTransaction,
    old_id: &str,
    new_id: &str,
) -> Result<(), String> {
    let mut table = write_txn
        .open_table(CONNECTIONS)
        .map_err(|error| format!("failed to open connections table: {error}"))?;
    let mut changed = Vec::new();
    for row in table
        .iter()
        .map_err(|error| format!("failed to iterate connections: {error}"))?
    {
        let (key, value) =
            row.map_err(|error| format!("failed to read connection row: {error}"))?;
        let mut entry =
            super::connection_store::decode_connection_entry(key.value(), value.value())?;
        let mut touched = false;
        if connection_saved_credential_id(&entry.connection).as_deref() == Some(old_id) {
            set_connection_saved_credential_id(&mut entry.connection, Some(new_id.to_string()));
            touched = true;
        }
        if let Some(jump_hosts) = replace_jump_host_credential_references(
            connection_jump_hosts(&entry.connection).as_deref(),
            old_id,
            new_id,
        )? {
            set_connection_jump_hosts(&mut entry.connection, jump_hosts);
            touched = true;
        }
        if touched {
            changed.push((key.value().to_string(), entry));
        }
    }
    for (id, entry) in changed {
        let bytes = encode_record(&format!("connection '{id}'"), &entry)?;
        table
            .insert(id.as_str(), bytes.as_slice())
            .map_err(|error| format!("failed to update connection '{id}': {error}"))?;
    }
    Ok(())
}

impl Store {
    pub fn clear_credential_references(&self, credential_id: &str) -> Result<usize, String> {
        let write_txn = self.database.begin_write().map_err(|error| {
            format!("failed to start credential reference cleanup transaction: {error}")
        })?;
        let mut cleared = 0;
        {
            let mut table = write_txn
                .open_table(CONNECTIONS)
                .map_err(|error| format!("failed to open connections table: {error}"))?;
            let mut changed = Vec::new();
            for row in table
                .iter()
                .map_err(|error| format!("failed to iterate connections: {error}"))?
            {
                let (key, value) =
                    row.map_err(|error| format!("failed to read connection row: {error}"))?;
                let mut entry =
                    super::connection_store::decode_connection_entry(key.value(), value.value())?;
                let mut touched = false;
                if connection_saved_credential_id(&entry.connection).as_deref()
                    == Some(credential_id)
                {
                    set_connection_saved_credential_id(&mut entry.connection, None);
                    touched = true;
                    cleared += 1;
                }
                if let Some((jump_hosts, count)) = clear_jump_host_credential_references(
                    connection_jump_hosts(&entry.connection).as_deref(),
                    credential_id,
                )? {
                    set_connection_jump_hosts(&mut entry.connection, jump_hosts);
                    touched = true;
                    cleared += count;
                }
                if touched {
                    changed.push((key.value().to_string(), entry));
                }
            }
            for (id, entry) in changed {
                let bytes = encode_record(&format!("connection '{id}'"), &entry)?;
                table
                    .insert(id.as_str(), bytes.as_slice())
                    .map_err(|error| format!("failed to update connection '{id}': {error}"))?;
            }
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit credential reference cleanup: {error}"))?;
        Ok(cleared)
    }

    pub fn delete_credential(&self, id: &str) -> Result<bool, String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start credential delete transaction: {error}"))?;
        let deleted = {
            let mut table = write_txn
                .open_table(CREDENTIALS)
                .map_err(|error| format!("failed to open credentials table: {error}"))?;
            let removed = table
                .remove(id)
                .map_err(|error| format!("failed to delete credential '{id}': {error}"))?;
            removed.is_some()
        };
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit credential delete: {error}"))?;
        Ok(deleted)
    }

    pub fn credential_connection_names(&self, credential_id: &str) -> Result<Vec<String>, String> {
        let mut names = self
            .credential_usages()?
            .into_iter()
            .filter(|usage| usage.credential_id == credential_id)
            .map(|usage| usage.connection_name)
            .collect::<Vec<_>>();
        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup();
        Ok(names)
    }

    pub fn credential_usages(&self) -> Result<Vec<StoredCredentialUsage>, String> {
        let read_txn = self.database.begin_read().map_err(|error| {
            format!("failed to start credential usage read transaction: {error}")
        })?;
        let table = read_txn
            .open_table(CONNECTIONS)
            .map_err(|error| format!("failed to open connections table: {error}"))?;
        let mut usages = Vec::new();
        for row in table
            .iter()
            .map_err(|error| format!("failed to iterate connections: {error}"))?
        {
            let (key, value) =
                row.map_err(|error| format!("failed to read connection row: {error}"))?;
            let entry =
                super::connection_store::decode_connection_entry(key.value(), value.value())?;
            if let Some(id) = connection_saved_credential_id(&entry.connection)
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                usages.push(StoredCredentialUsage {
                    credential_id: id.to_string(),
                    connection_id: key.value().to_string(),
                    connection_name: entry.connection.name.clone(),
                    relation: "connection".to_string(),
                });
            }
            for id in jump_host_credential_ids(connection_jump_hosts(&entry.connection).as_deref())?
            {
                usages.push(StoredCredentialUsage {
                    credential_id: id,
                    connection_id: key.value().to_string(),
                    connection_name: entry.connection.name.clone(),
                    relation: "jumpHost".to_string(),
                });
            }
        }
        usages.sort_by(|a, b| {
            a.connection_name
                .to_ascii_lowercase()
                .cmp(&b.connection_name.to_ascii_lowercase())
                .then_with(|| a.credential_id.cmp(&b.credential_id))
                .then_with(|| a.relation.cmp(&b.relation))
        });
        Ok(usages)
    }

    pub fn delete_unused_credentials(&self) -> Result<Vec<String>, String> {
        let used_ids = self
            .credential_usages()?
            .into_iter()
            .map(|usage| usage.credential_id)
            .collect::<HashSet<_>>();
        let unused_ids = self
            .credentials()?
            .into_iter()
            .filter(|credential| !used_ids.contains(&credential.id))
            .map(|record| record.id)
            .collect::<Vec<_>>();

        for id in &unused_ids {
            self.delete_credential(id)?;
        }

        Ok(unused_ids)
    }
}

fn jump_host_credential_ids(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let Ok(parsed) = serde_json::from_str::<Vec<StoredJumpHostCredentialRef>>(raw) else {
        return Ok(Vec::new());
    };
    Ok(parsed
        .into_iter()
        .filter_map(|hop| {
            hop.saved_credential_id
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
        })
        .collect())
}

fn replace_jump_host_credential_references(
    raw: Option<&str>,
    old_id: &str,
    new_id: &str,
) -> Result<Option<Option<String>>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Ok(None);
    };
    let Some(hops) = value.as_array_mut() else {
        return Ok(None);
    };
    let mut changed = false;
    for hop in hops {
        if hop
            .get("savedCredentialId")
            .and_then(|credential_id| credential_id.as_str())
            == Some(old_id)
        {
            hop["savedCredentialId"] = serde_json::Value::String(new_id.to_string());
            changed = true;
        }
    }
    if changed {
        serde_json::to_string(&value)
            .map(Some)
            .map(Some)
            .map_err(|error| {
                format!("failed to serialize jump host credential references: {error}")
            })
    } else {
        Ok(None)
    }
}

fn clear_jump_host_credential_references(
    raw: Option<&str>,
    credential_id: &str,
) -> Result<Option<(Option<String>, usize)>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Ok(None);
    };
    let Some(hops) = value.as_array_mut() else {
        return Ok(None);
    };
    let mut cleared = 0;
    for hop in hops {
        if hop
            .get("savedCredentialId")
            .and_then(|credential_id| credential_id.as_str())
            == Some(credential_id)
        {
            if let Some(object) = hop.as_object_mut() {
                object.remove("savedCredentialId");
            }
            cleared += 1;
        }
    }
    if cleared > 0 {
        serde_json::to_string(&value)
            .map(|serialized| Some((Some(serialized), cleared)))
            .map_err(|error| {
                format!("failed to serialize jump host credential references: {error}")
            })
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::{ConnectionOptions, SshConnectionDetails};
    use std::path::PathBuf;

    fn temp_store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("xterm-store-test-{}", crate::ids::new_id()));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let store = Store::open(&dir).expect("temp store should open");
        (store, dir)
    }

    fn password_credential(name: &str) -> StoredCredential {
        StoredCredential {
            position: 0,
            cred_type: "password".to_string(),
            name: name.to_string(),
            password: Some("secret".to_string()),
            private_key: None,
            passphrase: None,
        }
    }

    fn ssh_connection(saved_credential_id: Option<&str>) -> StoredConnection {
        StoredConnection {
            protocol: "ssh".to_string(),
            port: None,
            name: "conn".to_string(),
            host: "example.test".to_string(),
            user: "root".to_string(),
            options: ConnectionOptions::default(),
            details: ConnectionDetails::Ssh(SshConnectionDetails {
                auth_method: None,
                saved_credential_id: saved_credential_id.map(str::to_string),
                jump_hosts: saved_credential_id
                    .map(|id| format!("[{{\"host\":\"bastion\",\"savedCredentialId\":\"{id}\"}}]")),
            }),
        }
    }

    #[test]
    fn insert_appends_credentials_in_order() {
        let (store, dir) = temp_store();
        store
            .insert_credential("id-a", &password_credential("a"))
            .unwrap();
        store
            .insert_credential("id-b", &password_credential("b"))
            .unwrap();

        let ids = store
            .credentials()
            .unwrap()
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["id-a", "id-b"]);

        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_with_renamed_id_rewrites_references_atomically() {
        let (mut store, dir) = temp_store();
        store
            .insert_credential("id-a", &password_credential("a"))
            .unwrap();
        store
            .insert_credential("id-b", &password_credential("b"))
            .unwrap();
        store
            .insert_connection("conn-1", &ssh_connection(Some("id-a")))
            .unwrap();

        store
            .update_credential("id-a", "id-a2", &password_credential("a-renamed"))
            .unwrap();

        assert!(store.credential_by_id("id-a").unwrap().is_none());
        let renamed = store.credential_by_id("id-a2").unwrap().unwrap();
        assert_eq!(renamed.name, "a-renamed");
        assert_eq!(renamed.position, 0, "rename keeps the original position");

        let connection = store.connection_by_id("conn-1").unwrap().unwrap();
        assert_eq!(
            connection_saved_credential_id(&connection).as_deref(),
            Some("id-a2")
        );
        let jump_hosts = connection_jump_hosts(&connection).unwrap();
        assert!(jump_hosts.contains("id-a2"), "jump host refs rewritten");
        assert!(!jump_hosts.contains("id-a\""), "old id fully replaced");

        // Untouched credentials keep their position and order.
        let ids = store
            .credentials()
            .unwrap()
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["id-a2", "id-b"]);

        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_with_same_id_leaves_references_alone() {
        let (mut store, dir) = temp_store();
        store
            .insert_credential("id-a", &password_credential("a"))
            .unwrap();
        store
            .insert_connection("conn-1", &ssh_connection(Some("id-a")))
            .unwrap();

        store
            .update_credential("id-a", "id-a", &password_credential("a-new-name"))
            .unwrap();

        let credential = store.credential_by_id("id-a").unwrap().unwrap();
        assert_eq!(credential.name, "a-new-name");
        let connection = store.connection_by_id("conn-1").unwrap().unwrap();
        assert_eq!(
            connection_saved_credential_id(&connection).as_deref(),
            Some("id-a")
        );

        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
