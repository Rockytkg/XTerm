use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{
    logging,
    network_interface::{validate_bind_ip, DEFAULT_WILDCARD_BIND_IP},
    storage::repository::SettingsRepository,
};

pub(crate) const FILE_TRANSFER_EVENT: &str = "file-transfer";
pub(crate) const FILE_SERVICE_CONFIG_EVENT: &str = "file-service-config";
pub(crate) const DEFAULT_FILE_SERVICE_BIND_IP: &str = DEFAULT_WILDCARD_BIND_IP;
pub(crate) const DEFAULT_TFTP_PORT: u16 = 69;
pub(crate) const FILE_SERVICE_BIND_IP_KEY: &str = "fileServiceBindIp";
pub(crate) const DEFAULT_SFTP_PORT: u16 = 22;
pub(crate) const DEFAULT_FTP_PORT: u16 = 21;
pub(crate) const DEFAULT_FTP_PASSIVE_START: u16 = 50000;
pub(crate) const DEFAULT_FTP_PASSIVE_END: u16 = 50100;
pub(crate) const FILE_SERVICE_PROTOCOL_KEY: &str = "fileServiceProtocol";
pub(crate) const FILE_SERVICE_SHARED_DIR_KEY: &str = "fileServiceSharedDir";
pub(crate) const FILE_SERVICE_USERNAME_KEY: &str = "fileServiceUsername";
pub(crate) const FILE_SERVICE_PASSWORD_KEY: &str = "fileServicePassword";

/// Listen ports are fixed to the protocol defaults (TFTP 69, FTP 21,
/// SFTP 22); Linux binds privileged listeners through the elevation helper.
pub(crate) fn default_port(protocol: &str) -> u16 {
    match protocol {
        "ftp" => DEFAULT_FTP_PORT,
        "sftp" => DEFAULT_SFTP_PORT,
        _ => DEFAULT_TFTP_PORT,
    }
}

/// Shared pre-start validation for the authenticated services (FTP/SFTP):
/// a username is required.
pub(crate) fn validate_service_config(
    service: &str,
    config: &FileServiceConfig,
) -> Result<(), String> {
    if config.username.trim().is_empty() {
        return Err(format!("{service} username is required."));
    }
    Ok(())
}

pub(crate) fn parse_bind_address(
    service: &str,
    bind_ip: &str,
    port: u16,
) -> Result<SocketAddr, String> {
    let ip = bind_ip
        .parse()
        .map_err(|_| format!("invalid {service} bind IP address '{bind_ip}'"))?;
    Ok(SocketAddr::new(ip, port))
}

pub(crate) async fn canonical_shared_dir(service: &str, path: &str) -> Result<PathBuf, String> {
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|error| format!("failed to resolve {service} shared directory: {error}"))?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("The {service} shared path is not a directory."))
    }
}

/// Waits for a service accept task to finish after shutdown was signalled,
/// aborting it once `drain_timeout` elapses so a stuck task cannot hang the
/// stop command forever.
pub(crate) async fn await_runtime_task(
    service: &str,
    drain_timeout: Duration,
    mut task: tauri::async_runtime::JoinHandle<()>,
) -> Result<(), String> {
    tokio::select! {
        result = &mut task => result.map_err(|error| format!("{service} server task failed: {error}")),
        _ = tokio::time::sleep(drain_timeout) => {
            task.abort();
            let _ = task.await;
            Err(format!("timed out stopping {service} server; the server task was aborted"))
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FileServiceConfig {
    pub protocol: String,
    pub bind_ip: String,
    pub port: u16,
    pub shared_dir: String,
    pub username: String,
    pub password: String,
    pub running: bool,
}

/// Frontend-facing view of the file service configuration. Identical to
/// [`FileServiceConfig`] but without the password: the secret never leaves
/// the backend, the frontend only learns whether a non-default password is
/// configured via `passwordSet`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileServicePublicConfig {
    pub protocol: String,
    pub bind_ip: String,
    pub port: u16,
    pub shared_dir: String,
    pub username: String,
    pub password_set: bool,
    pub running: bool,
}

impl FileServicePublicConfig {
    pub(crate) fn from_config(config: &FileServiceConfig) -> Self {
        Self {
            protocol: config.protocol.clone(),
            bind_ip: config.bind_ip.clone(),
            port: config.port,
            shared_dir: config.shared_dir.clone(),
            username: config.username.clone(),
            password_set: super::password::is_explicit_password(&config.password),
            running: config.running,
        }
    }
}

impl Default for FileServiceConfig {
    fn default() -> Self {
        Self {
            protocol: "tftp".to_string(),
            bind_ip: DEFAULT_FILE_SERVICE_BIND_IP.to_string(),
            port: DEFAULT_TFTP_PORT,
            shared_dir: default_shared_dir(),
            username: "admin".to_string(),
            password: "admin".to_string(),
            running: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileTransferEvent {
    pub transfer_id: String,
    pub direction: String,
    pub name: String,
    pub peer: String,
    pub transferred: u64,
    pub total: u64,
    pub started_at_ms: u128,
    pub updated_at_ms: u128,
    pub done: bool,
    pub error: Option<String>,
}

pub(crate) struct FileServiceSettings {
    pub(crate) config: FileServiceConfig,
}

/// Reads a single setting, logging a read failure (key only, never the
/// value) and returning `None` so the caller falls back to its default.
fn setting_value_or(store: &impl SettingsRepository, key: &str) -> Option<String> {
    match store.setting_value(key) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                target: "file_service.settings",
                "failed to read setting '{key}': {error}; falling back to default"
            );
            None
        }
    }
}

impl FileServiceSettings {
    pub(crate) fn from_store(store: &impl SettingsRepository) -> Self {
        let bind_ip = setting_value_or(store, FILE_SERVICE_BIND_IP_KEY)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_FILE_SERVICE_BIND_IP.to_string());
        let protocol = setting_value_or(store, FILE_SERVICE_PROTOCOL_KEY)
            .filter(|value| value == "tftp" || value == "ftp" || value == "sftp")
            .unwrap_or_else(|| "tftp".to_string());
        let username = setting_value_or(store, FILE_SERVICE_USERNAME_KEY)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "admin".to_string());
        let password =
            super::password::resolve_password(store, &super::password::KeyringPasswordVault);
        let shared_dir = setting_value_or(store, FILE_SERVICE_SHARED_DIR_KEY)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(default_shared_dir);
        let port = default_port(&protocol);
        Self {
            config: FileServiceConfig {
                protocol: protocol.clone(),
                bind_ip,
                port,
                shared_dir,
                username,
                password,
                running: false,
            },
        }
    }

    pub(crate) fn config_snapshot(&self, running: bool) -> FileServiceConfig {
        FileServiceConfig {
            running,
            ..self.config.clone()
        }
    }
}

fn default_shared_dir() -> String {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[derive(Default, Debug)]
struct TransferState {
    direction: String,
    name: String,
    peer: String,
    transferred: u64,
    total: u64,
    started_at_ms: u128,
    updated_at_ms: u128,
}

#[derive(Debug)]
pub(crate) struct TransferRegistry {
    active_transfers: AtomicU64,
    completed_transfers: AtomicU64,
    failed_transfers: AtomicU64,
    transfers: parking_lot::Mutex<HashMap<String, TransferState>>,
}

impl TransferRegistry {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            active_transfers: AtomicU64::new(0),
            completed_transfers: AtomicU64::new(0),
            failed_transfers: AtomicU64::new(0),
            transfers: parking_lot::Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn start_transfer(
        &self,
        transfer_id: &str,
        direction: &str,
        name: &str,
        peer: &str,
        total: u64,
    ) {
        let now = crate::unix_timestamp_millis();
        let replaced = self.transfers.lock().insert(
            transfer_id.to_string(),
            TransferState {
                direction: direction.to_string(),
                name: name.to_string(),
                peer: peer.to_string(),
                total,
                started_at_ms: now,
                updated_at_ms: now,
                ..Default::default()
            },
        );
        if replaced.is_none() {
            self.active_transfers.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_progress(
        &self,
        transfer_id: &str,
        bytes: u64,
    ) -> Option<FileTransferEvent> {
        if bytes == 0 {
            return self.transfer_event(transfer_id, false, None);
        }
        let mut transfers = self.transfers.lock();
        let transfer = transfers.get_mut(transfer_id)?;
        transfer.transferred = transfer.transferred.saturating_add(bytes);
        transfer.updated_at_ms = crate::unix_timestamp_millis();
        Some(Self::event_from_state(transfer_id, transfer, false, None))
    }

    pub(crate) fn finish_transfer(
        &self,
        transfer_id: &str,
        error: Option<String>,
    ) -> Option<FileTransferEvent> {
        let mut transfer = self.transfers.lock().remove(transfer_id)?;
        self.active_transfers.fetch_sub(1, Ordering::Relaxed);
        transfer.updated_at_ms = crate::unix_timestamp_millis();
        if error.is_some() {
            self.failed_transfers.fetch_add(1, Ordering::Relaxed);
        } else {
            self.completed_transfers.fetch_add(1, Ordering::Relaxed);
        }
        if transfer.total < transfer.transferred {
            transfer.total = transfer.transferred;
        }
        Some(Self::event_from_state(
            transfer_id,
            &transfer,
            error.is_none(),
            error,
        ))
    }

    pub(crate) fn transfer_event(
        &self,
        transfer_id: &str,
        done: bool,
        error: Option<String>,
    ) -> Option<FileTransferEvent> {
        let transfers = self.transfers.lock();
        let transfer = transfers.get(transfer_id)?;
        Some(Self::event_from_state(transfer_id, transfer, done, error))
    }

    fn event_from_state(
        transfer_id: &str,
        transfer: &TransferState,
        done: bool,
        error: Option<String>,
    ) -> FileTransferEvent {
        FileTransferEvent {
            transfer_id: transfer_id.to_string(),
            direction: transfer.direction.clone(),
            name: transfer.name.clone(),
            peer: transfer.peer.clone(),
            transferred: transfer.transferred,
            total: transfer.total,
            started_at_ms: transfer.started_at_ms,
            updated_at_ms: transfer.updated_at_ms,
            done,
            error,
        }
    }
}

pub(crate) async fn validate_file_service_config(config: &FileServiceConfig) -> Result<(), String> {
    validate_bind_ip(&config.bind_ip)?;
    if config.shared_dir.trim().is_empty() {
        return Err("Choose a shared directory before starting the file service.".to_string());
    }
    let metadata = tokio::fs::metadata(&config.shared_dir)
        .await
        .map_err(|error| format!("failed to read shared directory: {error}"))?;
    if !metadata.is_dir() {
        return Err("The shared path is not a directory.".to_string());
    }
    Ok(())
}

pub(crate) fn emit_file_transfer(app: &AppHandle, event: FileTransferEvent) {
    if let Err(error) = app.emit(FILE_TRANSFER_EVENT, event) {
        logging::event("file_service.events", "file_service.transfer.emit_failed")
            .field("error", error)
            .warn();
    }
}

pub(crate) fn emit_file_service_config(app: &AppHandle, mut config: FileServiceConfig) {
    config.running = false;
    let public = FileServicePublicConfig::from_config(&config);
    if let Err(error) = app.emit(FILE_SERVICE_CONFIG_EVENT, public) {
        logging::event("file_service.events", "file_service.config.emit_failed")
            .field("error", error)
            .warn();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_service_config, FileServiceConfig, FileServicePublicConfig, TransferRegistry,
    };

    #[test]
    fn public_config_never_serializes_the_password() {
        let config = FileServiceConfig {
            password: "s3cret".to_string(),
            running: true,
            ..Default::default()
        };

        let public = FileServicePublicConfig::from_config(&config);
        let raw = serde_json::to_value(&public).expect("public config should serialize");

        assert!(raw.get("password").is_none());
        assert!(raw.get("passwordSet").is_some());
        assert_eq!(raw["passwordSet"], true);
        assert_eq!(raw["running"], true);
        assert_eq!(raw["protocol"], config.protocol);
    }

    #[test]
    fn default_password_is_reported_as_not_set() {
        let config = FileServiceConfig::default();
        let public = FileServicePublicConfig::from_config(&config);
        assert!(!public.password_set);
    }

    #[test]
    fn transfer_completion_is_idempotent() {
        let registry = TransferRegistry::new();
        registry.start_transfer("transfer", "read", "file.bin", "peer", 10);
        registry.record_progress("transfer", 4);

        let event = registry
            .finish_transfer("transfer", Some("connection lost".to_string()))
            .expect("active transfer should produce a terminal event");
        assert_eq!(event.transferred, 4);
        assert_eq!(event.error.as_deref(), Some("connection lost"));
        assert!(registry.finish_transfer("transfer", None).is_none());
    }

    #[test]
    fn validate_service_config_rejects_blank_username() {
        let config = FileServiceConfig {
            username: "  ".to_string(),
            ..Default::default()
        };
        assert!(validate_service_config("SFTP", &config).is_err());
    }

    #[test]
    fn restarting_the_same_transfer_id_does_not_duplicate_completion() {
        let registry = TransferRegistry::new();
        registry.start_transfer("transfer", "read", "old.bin", "peer", 10);
        registry.start_transfer("transfer", "write", "new.bin", "peer", 20);

        let event = registry
            .finish_transfer("transfer", None)
            .expect("replacement transfer should remain active");
        assert_eq!(event.direction, "write");
        assert_eq!(event.name, "new.bin");
        assert!(registry.finish_transfer("transfer", None).is_none());
    }
}
