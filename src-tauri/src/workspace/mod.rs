use serde::{Deserialize, Serialize};

use crate::credentials::{credential_metadata_by_id_in_store, CredentialMetadata};
use crate::ids;
use crate::logging;
use crate::state::AppState;
use crate::storage::{ConnectionOptions, Store, StoredConnection, StoredConnectionRecord};
use crate::terminal::domain::ProtocolKind;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpHostHop {
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub auth_method: Option<String>,
    #[serde(default)]
    pub saved_credential_id: Option<String>,
}

/// Protocol-agnostic options shared by all connection kinds. Kept separate
/// from protocol-specific details so new protocols can be added without
/// growing the profile struct.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfileOptions {
    pub(crate) terminal_type: Option<String>,
    pub(crate) encoding: Option<String>,
    pub(crate) backspace_sends: Option<String>,
    pub(crate) realtime_encoding_detection: Option<bool>,
    pub(crate) terminal_highlight_enabled: Option<bool>,
    pub(crate) terminal_more_prompt_cleanup: Option<bool>,
    /// 运行时指标采集开关；SSH 默认开启。仅支持单通道 shell 的设备
    ///（部分交换机）需要关闭，否则 exec 探测会触发远端断开整个会话。
    pub(crate) runtime_metrics: Option<bool>,
}

/// Protocol-specific details for a connection profile.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum ConnectionProfileDetails {
    Ssh(ConnectionProfileSshDetails),
    Telnet(ConnectionProfileTelnetDetails),
    Serial(ConnectionProfileSerialDetails),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfileSshDetails {
    pub(crate) auth_method: Option<String>,
    pub(crate) saved_credential_id: Option<String>,
    pub(crate) jump_hosts: Option<Vec<JumpHostHop>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfileTelnetDetails {
    pub(crate) auth_method: Option<String>,
    pub(crate) saved_credential_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfileSerialDetails {
    pub(crate) auth_method: Option<String>,
    pub(crate) saved_credential_id: Option<String>,
    pub(crate) baud_rate: Option<ConnectionBaudRate>,
    pub(crate) serial_quick_auto_baud: Option<bool>,
    pub(crate) data_bits: Option<u8>,
    pub(crate) flow_control: Option<String>,
    pub(crate) parity: Option<String>,
    pub(crate) stop_bits: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub(crate) id: String,
    pub(crate) protocol: String,
    pub(crate) port: Option<String>,
    pub(crate) name: String,
    pub(crate) host: String,
    pub(crate) user: String,
    pub(crate) options: ConnectionProfileOptions,
    pub(crate) details: ConnectionProfileDetails,
    #[serde(default, skip_serializing)]
    password: Option<String>,
    #[serde(default, skip_serializing)]
    key_path: Option<String>,
    #[serde(default, skip_serializing)]
    key_passphrase: Option<String>,
}

impl ConnectionProfile {
    fn ssh_details(&self) -> Option<&ConnectionProfileSshDetails> {
        match &self.details {
            ConnectionProfileDetails::Ssh(details) => Some(details),
            _ => None,
        }
    }

    #[allow(dead_code)]
    fn serial_details(&self) -> Option<&ConnectionProfileSerialDetails> {
        match &self.details {
            ConnectionProfileDetails::Serial(details) => Some(details),
            _ => None,
        }
    }

    pub(crate) fn auth_method(&self) -> Option<&str> {
        match &self.details {
            ConnectionProfileDetails::Ssh(details) => details.auth_method.as_deref(),
            ConnectionProfileDetails::Telnet(details) => details.auth_method.as_deref(),
            ConnectionProfileDetails::Serial(details) => details.auth_method.as_deref(),
        }
    }

    pub(crate) fn saved_credential_id(&self) -> Option<&str> {
        match &self.details {
            ConnectionProfileDetails::Ssh(details) => details.saved_credential_id.as_deref(),
            ConnectionProfileDetails::Telnet(details) => details.saved_credential_id.as_deref(),
            ConnectionProfileDetails::Serial(details) => details.saved_credential_id.as_deref(),
        }
    }

    pub(crate) fn jump_hosts(&self) -> Option<&Vec<JumpHostHop>> {
        self.ssh_details()
            .and_then(|details| details.jump_hosts.as_ref())
    }

    pub(crate) fn baud_rate(&self) -> Option<&ConnectionBaudRate> {
        self.serial_details()
            .and_then(|details| details.baud_rate.as_ref())
    }

    pub(crate) fn serial_quick_auto_baud(&self) -> Option<bool> {
        self.serial_details()
            .and_then(|details| details.serial_quick_auto_baud)
    }

    pub(crate) fn data_bits(&self) -> Option<u8> {
        self.serial_details().and_then(|details| details.data_bits)
    }

    pub(crate) fn flow_control(&self) -> Option<&str> {
        self.serial_details()
            .and_then(|details| details.flow_control.as_deref())
    }

    pub(crate) fn parity(&self) -> Option<&str> {
        self.serial_details()
            .and_then(|details| details.parity.as_deref())
    }

    pub(crate) fn stop_bits(&self) -> Option<u8> {
        self.serial_details().and_then(|details| details.stop_bits)
    }

    fn ssh_details_mut(&mut self) -> Option<&mut ConnectionProfileSshDetails> {
        match &mut self.details {
            ConnectionProfileDetails::Ssh(details) => Some(details),
            _ => None,
        }
    }

    fn serial_details_mut(&mut self) -> Option<&mut ConnectionProfileSerialDetails> {
        match &mut self.details {
            ConnectionProfileDetails::Serial(details) => Some(details),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConnectionBaudRate {
    Auto(String),
    Rate(u32),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionListItem {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: Option<String>,
    pub user: String,
    pub baud_rate: Option<ConnectionBaudRate>,
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
    pub realtime_encoding_detection: Option<bool>,
    pub terminal_highlight_enabled: Option<bool>,
    pub serial_quick_auto_baud: Option<bool>,
    pub data_bits: Option<u8>,
    pub flow_control: Option<String>,
    pub parity: Option<String>,
    pub stop_bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_hosts: Option<Vec<JumpHostHop>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backspace_sends: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_more_prompt_cleanup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_metrics: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceTab {
    id: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBootstrap {
    connections: Vec<ConnectionListItem>,
    active_sessions: Vec<String>,
    tabs: Vec<WorkspaceTab>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCredentialLinkInput {
    pub connection_id: String,
    pub credential_id: String,
}

#[tauri::command]
pub(crate) fn workspace_bootstrap(
    state: tauri::State<'_, AppState>,
) -> Result<WorkspaceBootstrap, String> {
    let connections: Vec<ConnectionListItem> = state
        .store()
        .connections()?
        .into_iter()
        .map(ConnectionListItem::from)
        .collect();
    logging::event("workspace.commands", "workspace.bootstrap")
        .field("connections", connections.len())
        .debug();
    Ok(WorkspaceBootstrap {
        connections,
        active_sessions: Vec::new(),
        tabs: default_tabs(),
    })
}

#[tauri::command]
pub(crate) fn connection_get(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<ConnectionProfile>, String> {
    logging::event("workspace.commands", "connection.get")
        .field("connection_id", &id)
        .trace();
    connection_profile_by_id(state.inner(), &id)
}

#[tauri::command]
pub(crate) fn connection_create(
    state: tauri::State<'_, AppState>,
    mut profile: ConnectionProfile,
) -> Result<String, String> {
    logging::event("workspace.commands", "connection.create.start")
        .field("protocol", &profile.protocol)
        .field("name", &profile.name)
        .info();
    normalize_connection_profile(state.inner(), &mut profile)?;
    profile.id = profile.id.trim().to_string();
    let mut store = state.store();
    if profile.id.is_empty() || store.connection_by_id(&profile.id)?.is_some() {
        profile.id = ids::new_id();
    }
    let stored = StoredConnection::from(&profile);
    store.insert_connection(&profile.id, &stored)?;
    logging::event("workspace.commands", "connection.create.success")
        .field("connection_id", &profile.id)
        .field("protocol", &profile.protocol)
        .info();
    Ok(profile.id)
}

#[tauri::command]
pub(crate) fn connection_update(
    state: tauri::State<'_, AppState>,
    id: String,
    mut profile: ConnectionProfile,
) -> Result<(), String> {
    logging::event("workspace.commands", "connection.update.start")
        .field("connection_id", &id)
        .field("requested_protocol", &profile.protocol)
        .info();
    profile.id = id;
    let requested_protocol = normalize_connection_protocol(&profile.protocol)?;
    let existing = {
        let store = state.store();
        store
            .connection_by_id(&profile.id)?
            .ok_or_else(|| format!("connection '{}' not found", profile.id))?
    };
    let current_protocol = normalize_connection_protocol(&existing.protocol)?;
    if requested_protocol != current_protocol {
        return Err("connection protocol cannot be changed".to_string());
    }
    profile.protocol = requested_protocol;
    normalize_connection_profile(state.inner(), &mut profile)?;
    let stored = StoredConnection::from(&profile);
    let store = state.store();
    let found = store.update_connection(&profile.id, &stored)?;
    if !found {
        return Err(format!("connection '{}' not found", profile.id));
    }
    logging::event("workspace.commands", "connection.update.success")
        .field("connection_id", &profile.id)
        .field("protocol", &profile.protocol)
        .info();
    Ok(())
}

#[tauri::command]
pub(crate) async fn connection_set_saved_credential(
    state: tauri::State<'_, AppState>,
    link: ConnectionCredentialLinkInput,
) -> Result<(), String> {
    let connection_id = link.connection_id.clone();
    let credential_id = link.credential_id.clone();
    state
        .inner()
        .run_store_blocking(move |store| {
            update_connection_credential_in_store(store, &connection_id, Some(&credential_id))
        })
        .await?;
    logging::event("workspace.commands", "connection.set_saved_credential")
        .field("connection_id", &link.connection_id)
        .field("credential_id", &link.credential_id)
        .info();
    Ok(())
}

#[tauri::command]
pub(crate) async fn connection_clear_saved_credential(
    state: tauri::State<'_, AppState>,
    connection_id: String,
) -> Result<(), String> {
    let connection_id_for_store = connection_id.clone();
    state
        .inner()
        .run_store_blocking(move |store| {
            update_connection_credential_in_store(store, &connection_id_for_store, None)
        })
        .await?;
    logging::event("workspace.commands", "connection.clear_saved_credential")
        .field("connection_id", &connection_id)
        .warn();
    Ok(())
}

#[tauri::command]
pub(crate) fn connection_reorder(
    state: tauri::State<'_, AppState>,
    order: Vec<String>,
) -> Result<(), String> {
    let mut store = state.store();
    store.reorder_connections(&order)?;
    logging::event("workspace.commands", "connection.reorder")
        .field("count", order.len())
        .info();
    Ok(())
}

#[tauri::command]
pub(crate) fn connection_delete(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let store = state.store();
    store.delete_connection(&id)?;
    store.delete_host_key(&id)?;
    logging::event("workspace.commands", "connection.delete")
        .field("connection_id", &id)
        .warn();
    Ok(())
}

pub(crate) fn workspace_connection_by_id(
    state: &AppState,
    connection_id: &str,
) -> Result<Option<ConnectionProfile>, String> {
    connection_profile_by_id(state, connection_id)
}

fn connection_profile_by_id(
    state: &AppState,
    connection_id: &str,
) -> Result<Option<ConnectionProfile>, String> {
    let store = state.store();
    let stored = store.connection_by_id(connection_id)?;
    stored
        .map(|stored| {
            let mut profile = connection_profile_from_stored(connection_id.to_string(), stored);
            normalize_connection_profile_in_store(&store, &mut profile)?;
            Ok(profile)
        })
        .transpose()
}

fn update_connection_credential_in_store(
    store: &mut Store,
    connection_id: &str,
    credential_id: Option<&str>,
) -> Result<(), String> {
    let mut profile = store
        .connection_by_id(connection_id)?
        .map(|stored| connection_profile_from_stored(connection_id.to_string(), stored))
        .ok_or_else(|| format!("connection '{connection_id}' not found"))?;
    if ProtocolKind::from_str(&profile.protocol).is_none() {
        return Err(
            "only SSH, Telnet, and serial connections can use saved credentials".to_string(),
        );
    }
    match &mut profile.details {
        ConnectionProfileDetails::Ssh(details) => {
            details.saved_credential_id = credential_id.map(str::to_string);
        }
        ConnectionProfileDetails::Telnet(details) => {
            details.saved_credential_id = credential_id.map(str::to_string);
        }
        ConnectionProfileDetails::Serial(details) => {
            details.saved_credential_id = credential_id.map(str::to_string);
        }
    }
    normalize_connection_profile_in_store(store, &mut profile)?;
    let stored = StoredConnection::from(&profile);
    if !store.update_connection(&profile.id, &stored)? {
        return Err(format!("connection '{}' not found", profile.id));
    }
    Ok(())
}

fn normalize_connection_profile_in_store(
    store: &Store,
    connection: &mut ConnectionProfile,
) -> Result<(), String> {
    let protocol = normalize_connection_protocol_kind(&connection.protocol)?;
    connection.protocol = protocol.as_str().to_string();
    clear_protocol_scoped_fields(connection);
    let credential = connection_credential_metadata(store, connection)?;
    apply_connection_defaults(
        connection,
        credential.as_ref().map(|value| value.cred_type()),
    );
    if protocol == ProtocolKind::Ssh && connection.user.trim().is_empty() {
        return Err("SSH username is required".to_string());
    }
    Ok(())
}

fn connection_credential_metadata(
    store: &Store,
    connection: &ConnectionProfile,
) -> Result<Option<CredentialMetadata>, String> {
    match connection.saved_credential_id() {
        Some(credential_id) if !credential_id.trim().is_empty() => {
            let credential = credential_metadata_by_id_in_store(store, credential_id)?
                .ok_or_else(|| format!("selected credential '{credential_id}' does not exist"))?;
            let protocol = ProtocolKind::from_str(&connection.protocol).ok_or_else(|| {
                format!("unsupported connection protocol '{}'", connection.protocol)
            })?;
            if protocol.requires_password_credential() && credential.cred_type() != "password" {
                return Err(format!(
                    "{} connections can only use saved password credentials",
                    connection.protocol
                ));
            }
            Ok(Some(credential))
        }
        _ => Ok(None),
    }
}

fn normalize_connection_profile(
    state: &AppState,
    connection: &mut ConnectionProfile,
) -> Result<(), String> {
    let protocol = normalize_connection_protocol_kind(&connection.protocol)?;
    connection.protocol = protocol.as_str().to_string();
    clear_protocol_scoped_fields(connection);
    let credential = connection_credential_metadata(&state.store(), connection)?;
    apply_connection_defaults(
        connection,
        credential.as_ref().map(|value| value.cred_type()),
    );
    if protocol == ProtocolKind::Ssh && connection.user.trim().is_empty() {
        return Err("SSH username is required".to_string());
    }
    Ok(())
}

fn normalize_connection_protocol(protocol: &str) -> Result<String, String> {
    normalize_connection_protocol_kind(protocol).map(|protocol| protocol.as_str().to_string())
}

fn normalize_connection_protocol_kind(protocol: &str) -> Result<ProtocolKind, String> {
    ProtocolKind::from_str(protocol)
        .ok_or_else(|| format!("unsupported connection protocol '{}'", protocol.trim()))
}

fn clear_protocol_scoped_fields(connection: &mut ConnectionProfile) {
    let protocol = ProtocolKind::from_str(&connection.protocol);
    if let Some(details) = connection.ssh_details_mut() {
        if protocol != Some(ProtocolKind::Ssh) {
            details.jump_hosts = None;
        }
    }
    if protocol.is_none() {
        match &mut connection.details {
            ConnectionProfileDetails::Ssh(details) => {
                details.auth_method = None;
                details.saved_credential_id = None;
            }
            ConnectionProfileDetails::Telnet(details) => {
                details.auth_method = None;
                details.saved_credential_id = None;
            }
            ConnectionProfileDetails::Serial(details) => {
                details.auth_method = None;
                details.saved_credential_id = None;
            }
        }
    }

    if let Some(details) = connection.serial_details_mut() {
        if protocol != Some(ProtocolKind::Serial) {
            details.baud_rate = None;
            details.serial_quick_auto_baud = None;
            details.data_bits = None;
            details.flow_control = None;
            details.parity = None;
            details.stop_bits = None;
        }
    }
}

fn apply_connection_defaults(connection: &mut ConnectionProfile, credential_type: Option<&str>) {
    let protocol = ProtocolKind::from_str(&connection.protocol);
    if protocol == Some(ProtocolKind::Serial) {
        if !matches!(connection.details, ConnectionProfileDetails::Serial(_)) {
            connection.details =
                ConnectionProfileDetails::Serial(ConnectionProfileSerialDetails::default());
        }
        if let Some(details) = connection.serial_details_mut() {
            if details.serial_quick_auto_baud.is_none() {
                details.serial_quick_auto_baud = Some(true);
            }
            details.data_bits = Some(details.data_bits.unwrap_or(8));
            details.flow_control = Some(normalize_serial_text_setting(
                details.flow_control.as_deref(),
                "none",
            ));
            details.parity = Some(normalize_serial_text_setting(
                details.parity.as_deref(),
                "none",
            ));
            details.stop_bits = Some(details.stop_bits.unwrap_or(1));
        }
    } else if let Some(details) = connection.serial_details_mut() {
        details.baud_rate = None;
        details.serial_quick_auto_baud = None;
        details.data_bits = None;
        details.flow_control = None;
        details.parity = None;
        details.stop_bits = None;
    }
    if let Some(credential_type) = credential_type {
        match &mut connection.details {
            ConnectionProfileDetails::Ssh(details) => {
                details.auth_method = Some(credential_type.to_string());
            }
            ConnectionProfileDetails::Telnet(details) => {
                details.auth_method = Some(credential_type.to_string());
            }
            ConnectionProfileDetails::Serial(details) => {
                details.auth_method = Some(credential_type.to_string());
            }
        }
    } else if ProtocolKind::from_str(&connection.protocol)
        .is_some_and(|protocol| protocol.requires_password_credential())
    {
        match &mut connection.details {
            ConnectionProfileDetails::Ssh(details) => {
                details.auth_method = Some("password".to_string());
            }
            ConnectionProfileDetails::Telnet(details) => {
                details.auth_method = Some("password".to_string());
            }
            ConnectionProfileDetails::Serial(details) => {
                details.auth_method = Some("password".to_string());
            }
        }
    }
    connection.password = None;
    connection.key_path = None;
    connection.key_passphrase = None;
}

fn normalize_serial_text_setting(value: Option<&str>, default: &str) -> String {
    let trimmed = value.unwrap_or(default).trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_ascii_lowercase()
    }
}

fn default_tabs() -> Vec<WorkspaceTab> {
    vec![
        WorkspaceTab { id: "shell" },
        WorkspaceTab { id: "files" },
        WorkspaceTab { id: "sftp" },
    ]
}

impl From<StoredConnectionRecord> for ConnectionListItem {
    fn from(record: StoredConnectionRecord) -> Self {
        flatten_stored_connection(record.id, record.connection)
    }
}

fn flatten_stored_connection(id: String, c: StoredConnection) -> ConnectionListItem {
    let (
        auth_method,
        saved_credential_id,
        baud_rate,
        serial_quick_auto_baud,
        data_bits,
        flow_control,
        parity,
        stop_bits,
        jump_hosts,
    ) = match c.details {
        crate::storage::ConnectionDetails::Ssh(details) => (
            details.auth_method,
            details.saved_credential_id,
            None,
            None,
            None,
            None,
            None,
            None,
            parse_jump_hosts(details.jump_hosts),
        ),
        crate::storage::ConnectionDetails::Telnet(details) => (
            details.auth_method,
            details.saved_credential_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ),
        crate::storage::ConnectionDetails::Serial(details) => (
            details.auth_method,
            details.saved_credential_id,
            details.baud_rate.map(|v| {
                if v > 0 {
                    ConnectionBaudRate::Rate(u32::try_from(v).unwrap_or(9600))
                } else {
                    ConnectionBaudRate::Auto("auto".to_string())
                }
            }),
            details.serial_quick_auto_baud,
            details.data_bits.and_then(|v| u8::try_from(v).ok()),
            details.flow_control,
            details.parity,
            details.stop_bits.and_then(|v| u8::try_from(v).ok()),
            None,
        ),
    };

    ConnectionListItem {
        id,
        name: c.name,
        protocol: c.protocol,
        host: c.host,
        port: c.port,
        user: c.user,
        baud_rate,
        auth_method,
        saved_credential_id,
        realtime_encoding_detection: c.options.realtime_encoding_detection,
        terminal_highlight_enabled: c.options.terminal_highlight_enabled,
        serial_quick_auto_baud,
        data_bits,
        flow_control,
        parity,
        stop_bits,
        jump_hosts,
        terminal_type: c.options.terminal_type,
        encoding: c.options.encoding,
        backspace_sends: c.options.backspace_sends,
        terminal_more_prompt_cleanup: c.options.terminal_more_prompt_cleanup,
        runtime_metrics: c.options.runtime_metrics,
    }
}

fn connection_profile_from_stored(id: String, c: StoredConnection) -> ConnectionProfile {
    let item = flatten_stored_connection(id, c);
    let options = ConnectionProfileOptions {
        terminal_type: item.terminal_type,
        encoding: item.encoding,
        backspace_sends: item.backspace_sends,
        realtime_encoding_detection: item.realtime_encoding_detection,
        terminal_highlight_enabled: item.terminal_highlight_enabled,
        terminal_more_prompt_cleanup: item.terminal_more_prompt_cleanup,
        runtime_metrics: item.runtime_metrics,
    };
    let details = match item.protocol.as_str() {
        "ssh" => ConnectionProfileDetails::Ssh(ConnectionProfileSshDetails {
            auth_method: item.auth_method,
            saved_credential_id: item.saved_credential_id,
            jump_hosts: item.jump_hosts,
        }),
        "serial" => ConnectionProfileDetails::Serial(ConnectionProfileSerialDetails {
            auth_method: item.auth_method,
            saved_credential_id: item.saved_credential_id,
            baud_rate: item.baud_rate,
            serial_quick_auto_baud: item.serial_quick_auto_baud,
            data_bits: item.data_bits,
            flow_control: item.flow_control,
            parity: item.parity,
            stop_bits: item.stop_bits,
        }),
        _ => ConnectionProfileDetails::Telnet(ConnectionProfileTelnetDetails {
            auth_method: item.auth_method,
            saved_credential_id: item.saved_credential_id,
        }),
    };
    ConnectionProfile {
        id: item.id,
        protocol: item.protocol,
        port: item.port,
        name: item.name,
        host: item.host,
        user: item.user,
        options,
        details,
        password: None,
        key_path: None,
        key_passphrase: None,
    }
}

impl From<&ConnectionProfile> for StoredConnection {
    fn from(c: &ConnectionProfile) -> Self {
        let options = ConnectionOptions {
            terminal_type: c.options.terminal_type.clone(),
            encoding: c.options.encoding.clone(),
            backspace_sends: c.options.backspace_sends.clone(),
            realtime_encoding_detection: c.options.realtime_encoding_detection,
            terminal_highlight_enabled: c.options.terminal_highlight_enabled,
            terminal_more_prompt_cleanup: c.options.terminal_more_prompt_cleanup,
            runtime_metrics: c.options.runtime_metrics,
        };

        let details = match c.protocol.as_str() {
            "ssh" => crate::storage::ConnectionDetails::Ssh(crate::storage::SshConnectionDetails {
                auth_method: c.auth_method().map(str::to_string),
                saved_credential_id: c.saved_credential_id().map(str::to_string),
                jump_hosts: serialize_jump_hosts(c.jump_hosts()),
            }),
            "telnet" => {
                crate::storage::ConnectionDetails::Telnet(crate::storage::TelnetConnectionDetails {
                    auth_method: c.auth_method().map(str::to_string),
                    saved_credential_id: c.saved_credential_id().map(str::to_string),
                })
            }
            "serial" => {
                crate::storage::ConnectionDetails::Serial(crate::storage::SerialConnectionDetails {
                    auth_method: c.auth_method().map(str::to_string),
                    saved_credential_id: c.saved_credential_id().map(str::to_string),
                    baud_rate: c.baud_rate().map(|v| match v {
                        ConnectionBaudRate::Rate(r) => i64::from(*r),
                        ConnectionBaudRate::Auto(_) => 0,
                    }),
                    serial_quick_auto_baud: c.serial_quick_auto_baud(),
                    data_bits: c.data_bits().map(i64::from),
                    flow_control: c.flow_control().map(str::to_string),
                    parity: c.parity().map(str::to_string),
                    stop_bits: c.stop_bits().map(i64::from),
                })
            }
            _ => {
                crate::storage::ConnectionDetails::Telnet(crate::storage::TelnetConnectionDetails {
                    auth_method: None,
                    saved_credential_id: None,
                })
            }
        };

        Self {
            protocol: c.protocol.clone(),
            port: c.port.clone(),
            name: c.name.clone(),
            host: c.host.clone(),
            user: c.user.clone(),
            options,
            details,
        }
    }
}

fn parse_jump_hosts(jump_hosts: Option<String>) -> Option<Vec<JumpHostHop>> {
    if let Some(raw) = jump_hosts {
        if let Ok(parsed) = serde_json::from_str::<Vec<JumpHostHop>>(&raw) {
            let cleaned = parsed
                .into_iter()
                .map(|hop| JumpHostHop {
                    connection_id: hop
                        .connection_id
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty()),
                    host: hop.host.trim().to_string(),
                    port: hop
                        .port
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty()),
                    user: hop
                        .user
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty()),
                    auth_method: hop
                        .auth_method
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty()),
                    saved_credential_id: hop
                        .saved_credential_id
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty()),
                })
                .filter(|hop| hop.connection_id.is_some() || !hop.host.is_empty())
                .collect::<Vec<_>>();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn serialize_jump_hosts(jump_hosts: Option<&Vec<JumpHostHop>>) -> Option<String> {
    let hops = jump_hosts?;
    let cleaned = hops
        .iter()
        .map(|hop| JumpHostHop {
            connection_id: hop
                .connection_id
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            host: hop.host.trim().to_string(),
            port: hop
                .port
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            user: hop
                .user
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            auth_method: hop
                .auth_method
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            saved_credential_id: hop
                .saved_credential_id
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
        })
        .filter(|hop| hop.connection_id.is_some() || !hop.host.is_empty())
        .collect::<Vec<_>>();
    if cleaned.is_empty() {
        None
    } else {
        serde_json::to_string(&cleaned).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionListItem, StoredConnection, StoredConnectionRecord};
    use crate::storage::{ConnectionDetails, ConnectionOptions, TelnetConnectionDetails};

    fn stored_connection() -> StoredConnection {
        StoredConnection {
            protocol: "telnet".to_string(),
            port: Some("23".to_string()),
            name: "legacy switch".to_string(),
            host: "10.0.0.1".to_string(),
            user: String::new(),
            options: ConnectionOptions {
                terminal_type: Some("vt100".to_string()),
                encoding: None,
                backspace_sends: None,
                realtime_encoding_detection: Some(false),
                terminal_highlight_enabled: Some(true),
                terminal_more_prompt_cleanup: Some(true),
                runtime_metrics: None,
            },
            details: ConnectionDetails::Telnet(TelnetConnectionDetails {
                auth_method: Some("password".to_string()),
                saved_credential_id: None,
            }),
        }
    }

    #[test]
    fn connection_list_item_includes_more_prompt_cleanup_flag() {
        let item = ConnectionListItem::from(StoredConnectionRecord {
            id: "conn-1".to_string(),
            connection: stored_connection(),
        });

        assert_eq!(item.terminal_more_prompt_cleanup, Some(true));
        assert_eq!(item.realtime_encoding_detection, Some(false));
    }
}
