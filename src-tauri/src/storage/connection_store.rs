use redb::{ReadableDatabase, ReadableTable};
use serde::{Deserialize, Serialize};

use super::schema::{decode_record, encode_record, CONNECTIONS};
use super::*;
use crate::logging;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct StoredConnectionEntry {
    pub(super) position: i64,
    pub(super) connection: StoredConnection,
}

pub(super) fn decode_connection_entry(
    id: &str,
    bytes: &[u8],
) -> Result<StoredConnectionEntry, String> {
    decode_record(&format!("connection '{id}'"), bytes)
}

impl Store {
    pub fn connections(&self) -> Result<Vec<StoredConnectionRecord>, String> {
        logging::event("storage.connection_store", "connections.list").debug();
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start connection read transaction: {error}"))?;
        let table = read_txn
            .open_table(CONNECTIONS)
            .map_err(|error| format!("failed to open connections table: {error}"))?;
        let mut entries = Vec::new();
        for row in table
            .iter()
            .map_err(|error| format!("failed to iterate connections: {error}"))?
        {
            let (key, value) =
                row.map_err(|error| format!("failed to read connection row: {error}"))?;
            entries.push((
                key.value().to_string(),
                decode_connection_entry(key.value(), value.value())?,
            ));
        }
        entries.sort_by(|a, b| {
            a.1.position
                .cmp(&b.1.position)
                .then_with(|| {
                    a.1.connection
                        .name
                        .to_ascii_lowercase()
                        .cmp(&b.1.connection.name.to_ascii_lowercase())
                })
                .then_with(|| a.0.cmp(&b.0))
        });
        let results = entries
            .into_iter()
            .map(|(id, entry)| StoredConnectionRecord {
                id,
                connection: entry.connection,
            })
            .collect::<Vec<_>>();
        logging::event("storage.connection_store", "connections.list.success")
            .field("count", results.len())
            .debug();
        Ok(results)
    }

    pub fn connection_by_id(&self, id: &str) -> Result<Option<StoredConnection>, String> {
        logging::event("storage.connection_store", "connection.get")
            .field("connection_id", id)
            .debug();
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start connection read transaction: {error}"))?;
        let table = read_txn
            .open_table(CONNECTIONS)
            .map_err(|error| format!("failed to open connections table: {error}"))?;
        let result = table
            .get(id)
            .map_err(|error| format!("failed to read connection '{id}': {error}"))?
            .map(|value| decode_connection_entry(id, value.value()).map(|entry| entry.connection))
            .transpose()?;
        logging::event("storage.connection_store", "connection.get.success")
            .field("connection_id", id)
            .field("found", result.is_some())
            .debug();
        Ok(result)
    }

    pub fn insert_connection(&mut self, id: &str, c: &StoredConnection) -> Result<(), String> {
        logging::event("storage.connection_store", "connection.insert")
            .field("connection_id", id)
            .field("protocol", &c.protocol)
            .info();
        let result = self.insert_connection_record(id, c);
        if let Err(error) = &result {
            logging::event("storage.connection_store", "connection.insert.failed")
                .field("connection_id", id)
                .field("error", error)
                .error();
        }
        result
    }

    fn insert_connection_record(&mut self, id: &str, c: &StoredConnection) -> Result<(), String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start connection write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(CONNECTIONS)
                .map_err(|error| format!("failed to open connections table: {error}"))?;
            if table
                .get(id)
                .map_err(|error| format!("failed to check connection '{id}': {error}"))?
                .is_some()
            {
                return Err(format!("connection '{id}' already exists"));
            }
            let mut next_position = 0;
            for row in table
                .iter()
                .map_err(|error| format!("failed to iterate connections: {error}"))?
            {
                let (key, value) =
                    row.map_err(|error| format!("failed to read connection row: {error}"))?;
                let entry = decode_connection_entry(key.value(), value.value())?;
                next_position = next_position.max(entry.position + 1);
            }
            let entry = StoredConnectionEntry {
                position: next_position,
                connection: c.clone(),
            };
            let bytes = encode_record(&format!("connection '{id}'"), &entry)?;
            table
                .insert(id, bytes.as_slice())
                .map_err(|error| format!("failed to insert connection '{id}': {error}"))?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit connection '{id}': {error}"))
    }

    pub fn update_connection(&self, id: &str, c: &StoredConnection) -> Result<bool, String> {
        logging::event("storage.connection_store", "connection.update")
            .field("connection_id", id)
            .field("protocol", &c.protocol)
            .info();
        let updated = match self.update_connection_record(id, c) {
            Ok(updated) => updated,
            Err(error) => {
                logging::event("storage.connection_store", "connection.update.failed")
                    .field("connection_id", id)
                    .field("error", &error)
                    .error();
                return Err(error);
            }
        };
        logging::event("storage.connection_store", "connection.update.success")
            .field("connection_id", id)
            .field("updated", updated)
            .debug();
        Ok(updated)
    }

    fn update_connection_record(&self, id: &str, c: &StoredConnection) -> Result<bool, String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start connection write transaction: {error}"))?;
        let updated = {
            let mut table = write_txn
                .open_table(CONNECTIONS)
                .map_err(|error| format!("failed to open connections table: {error}"))?;
            let current = table
                .get(id)
                .map_err(|error| format!("failed to read connection '{id}': {error}"))?
                .map(|existing| decode_connection_entry(id, existing.value()))
                .transpose()?;
            match current {
                None => false,
                Some(current) => {
                    let entry = StoredConnectionEntry {
                        position: current.position,
                        connection: c.clone(),
                    };
                    let bytes = encode_record(&format!("connection '{id}'"), &entry)?;
                    table
                        .insert(id, bytes.as_slice())
                        .map_err(|error| format!("failed to update connection '{id}': {error}"))?;
                    true
                }
            }
        };
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit connection '{id}': {error}"))?;
        Ok(updated)
    }

    pub fn reorder_connections(&mut self, ordered_ids: &[String]) -> Result<(), String> {
        logging::event("storage.connection_store", "connection.reorder")
            .field("count", ordered_ids.len())
            .info();
        let existing_ids = self
            .connections()?
            .into_iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        let existing = existing_ids.iter().cloned().collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut reordered = Vec::with_capacity(existing_ids.len());

        for id in ordered_ids {
            if !existing.contains(id) {
                return Err(format!("connection '{id}' not found"));
            }
            if !seen.insert(id.clone()) {
                return Err(format!("connection '{id}' appears more than once"));
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
            .map_err(|error| format!("failed to start connection reorder transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(CONNECTIONS)
                .map_err(|error| format!("failed to open connections table: {error}"))?;
            for (index, id) in reordered.iter().enumerate() {
                let Some(mut entry) = table
                    .get(id.as_str())
                    .map_err(|error| format!("failed to read connection '{id}': {error}"))?
                    .map(|current| decode_connection_entry(id, current.value()))
                    .transpose()?
                else {
                    return Err(format!("connection '{id}' not found"));
                };
                entry.position = index as i64;
                let bytes = encode_record(&format!("connection '{id}'"), &entry)?;
                table
                    .insert(id.as_str(), bytes.as_slice())
                    .map_err(|error| format!("failed to reorder connection '{id}': {error}"))?;
            }
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit connection reorder: {error}"))?;
        logging::event("storage.connection_store", "connection.reorder.success")
            .field("count", reordered.len())
            .debug();
        Ok(())
    }

    pub fn delete_connection(&self, id: &str) -> Result<bool, String> {
        logging::event("storage.connection_store", "connection.delete")
            .field("connection_id", id)
            .warn();
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start connection delete transaction: {error}"))?;
        let deleted = {
            let mut table = write_txn
                .open_table(CONNECTIONS)
                .map_err(|error| format!("failed to open connections table: {error}"))?;
            let removed = table
                .remove(id)
                .map_err(|error| format!("failed to delete connection '{id}': {error}"))?;
            removed.is_some()
        };
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit connection delete: {error}"))?;
        logging::event("storage.connection_store", "connection.delete.success")
            .field("connection_id", id)
            .field("deleted", deleted)
            .debug();
        Ok(deleted)
    }
}
