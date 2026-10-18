use std::path::Path;

use redb::Database;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};

use super::schema::initialize_schema;

pub(super) const DATABASE_FILE_NAME: &str = "data.redb";

/// Failure modes of opening the persistent store. `AlreadyOpen` is redb's
/// structured signal that another process holds the database file lock;
/// matching on redb's display text instead would silently break the
/// second-instance exit path on any redb wording change.
#[derive(Debug)]
pub enum StoreOpenError {
    AlreadyOpen,
    Failed(String),
}

pub struct Store {
    pub(super) database: Database,
}

impl Store {
    pub fn open(database_dir: &Path) -> Result<Self, StoreOpenError> {
        let database_path = database_dir.join(DATABASE_FILE_NAME);
        let database = Database::create(&database_path).map_err(|error| match error {
            redb::DatabaseError::DatabaseAlreadyOpen => StoreOpenError::AlreadyOpen,
            error => StoreOpenError::Failed(format!("failed to open Redb store: {error}")),
        })?;
        initialize_schema(&database).map_err(StoreOpenError::Failed)?;
        Ok(Self { database })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub enable_animations: bool,
    pub ui_font_size: i64,
    pub locale: String,
    pub show_latency: bool,
    pub proxy_toolbar_enabled: bool,
    pub file_service_toolbar_enabled: bool,
    pub serial_redetect_baud_shortcut: String,
    pub session_recording_shortcut: String,
    pub terminal_theme: String,
    pub terminal_theme_follow_app: bool,
    pub terminal_theme_light: String,
    pub terminal_theme_dark: String,
    pub terminal_font_family: String,
    pub terminal_font_size: i64,
    pub terminal_line_height: f64,
    pub editor_font_family: String,
    pub editor_font_size: i64,
    pub editor_tab_size: i64,
    pub editor_line_wrapping: bool,
    pub editor_highlight_active_line: bool,
    pub editor_theme_mode: String,
    pub terminal_scrollback: i64,
    pub terminal_cursor_blink: bool,
    pub terminal_cursor_style: String,
    pub terminal_cursor_inactive_style: String,
    pub terminal_cursor_width: i64,
    pub terminal_scroll_sensitivity: f64,
    pub terminal_fast_scroll_sensitivity: f64,
    pub terminal_smooth_scroll_duration: i64,
    pub terminal_alt_click_moves_cursor: bool,
    pub terminal_right_click_selects_word: bool,
    pub terminal_scroll_on_user_input: bool,
    pub terminal_scroll_on_erase_in_display: bool,
    pub terminal_draw_bold_text_in_bright_colors: bool,
    pub terminal_minimum_contrast_ratio: f64,
    pub terminal_custom_glyphs: bool,
    pub terminal_rescale_overlapping_glyphs: bool,
    pub terminal_mac_option_is_meta: bool,
    pub terminal_mac_option_click_forces_selection: bool,
    pub terminal_webgl: bool,
    pub terminal_trzsz: bool,
    pub transfer_drag_upload: bool,
    pub transfer_directory_upload: bool,
    pub transfer_max_chunk_size: i64,
    pub transfer_drag_init_timeout: i64,
    pub terminal_type: String,
    pub terminal_search_shortcut: String,
    // 前端/存储键约定为 openDevToolsShortcut；camelCase 分词会得到 openDevtoolsShortcut。
    #[serde(rename = "openDevToolsShortcut")]
    pub open_devtools_shortcut: String,
    #[serde(serialize_with = "serialize_terminal_highlight_schemes")]
    pub terminal_highlight_schemes: String,
    pub theme: String,
    pub credential_layout_mode: String,
    pub ui_theme_light: String,
    pub ui_theme_dark: String,
}

fn serialize_terminal_highlight_schemes<S>(raw: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let schemes = serde_json::from_str::<Vec<serde_json::Value>>(raw).unwrap_or_default();
    schemes.serialize(serializer)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredCredential {
    #[serde(default)]
    pub position: i64,
    pub cred_type: String,
    pub name: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredCredentialRecord {
    pub id: String,
    pub credential: StoredCredential,
}

/// Common session options persisted independently of the connection protocol.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ConnectionOptions {
    pub terminal_type: Option<String>,
    pub encoding: Option<String>,
    pub backspace_sends: Option<String>,
    pub realtime_encoding_detection: Option<bool>,
    pub terminal_highlight_enabled: Option<bool>,
    pub terminal_more_prompt_cleanup: Option<bool>,
    pub runtime_metrics: Option<bool>,
}

/// Protocol-specific details persisted for a connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum ConnectionDetails {
    Ssh(SshConnectionDetails),
    Telnet(TelnetConnectionDetails),
    Serial(SerialConnectionDetails),
    Vnc(VncConnectionDetails),
    Rdp(RdpConnectionDetails),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SshConnectionDetails {
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
    pub jump_hosts: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TelnetConnectionDetails {
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SerialConnectionDetails {
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
    pub baud_rate: Option<i64>,
    pub serial_quick_auto_baud: Option<bool>,
    pub data_bits: Option<i64>,
    pub flow_control: Option<String>,
    pub parity: Option<String>,
    pub stop_bits: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VncConnectionDetails {
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
    pub view_only: Option<bool>,
    pub shared: Option<bool>,
    pub quality: Option<i64>,
    pub compression: Option<i64>,
    pub scale_mode: Option<String>,
    pub clipboard_sync: Option<bool>,
    pub resize_session: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RdpConnectionDetails {
    pub auth_method: Option<String>,
    pub saved_credential_id: Option<String>,
    pub domain: Option<String>,
    pub scale_mode: Option<String>,
    pub clipboard_sync: Option<bool>,
    pub resize_session: Option<bool>,
}

/// Persisted connection record, written and read in a nested shape with
/// `options` and `details`.
#[derive(Clone, Debug, Serialize)]
pub struct StoredConnection {
    pub protocol: String,
    pub port: Option<String>,
    pub name: String,
    pub host: String,
    pub user: String,
    pub options: ConnectionOptions,
    pub details: ConnectionDetails,
}

impl<'de> Deserialize<'de> for StoredConnection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        let Some(obj) = value.as_object_mut() else {
            return Err(D::Error::custom("StoredConnection must be an object"));
        };

        let protocol = take_string(obj, "protocol")
            .ok_or_else(|| D::Error::custom("missing protocol field in StoredConnection"))?;
        let port = take_string(obj, "port");
        let name = take_string(obj, "name").unwrap_or_default();
        let host = take_string(obj, "host").unwrap_or_default();
        let user = take_string(obj, "user").unwrap_or_default();

        // The nested shape decodes leniently: a malformed field or object
        // degrades to defaults so the record still loads and can be repaired
        // in the UI, instead of failing the whole connection list.  An
        // unknown protocol stays a hard error (the list reader skips such
        // records with a warning).
        let options = obj
            .remove("options")
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let details = obj
            .remove("details")
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| default_details_for(&protocol))
            .ok_or_else(|| {
                D::Error::custom(format!("unsupported connection protocol '{protocol}'"))
            })?;

        Ok(StoredConnection {
            protocol,
            port,
            name,
            host,
            user,
            options,
            details,
        })
    }
}

/// All-default details for a known protocol: the nested-shape fallback when
/// the stored details object is missing or cannot be decoded.  Returns None
/// for unknown protocols, which stay a hard decode error.
fn default_details_for(protocol: &str) -> Option<ConnectionDetails> {
    match protocol {
        "ssh" => Some(ConnectionDetails::Ssh(SshConnectionDetails::default())),
        "telnet" => Some(ConnectionDetails::Telnet(TelnetConnectionDetails::default())),
        "serial" => Some(ConnectionDetails::Serial(SerialConnectionDetails::default())),
        "vnc" => Some(ConnectionDetails::Vnc(VncConnectionDetails::default())),
        "rdp" => Some(ConnectionDetails::Rdp(RdpConnectionDetails::default())),
        _ => None,
    }
}

fn take_string(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.remove(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_str().map(str::to_string)
        }
    })
}

#[derive(Clone, Debug)]
pub struct StoredConnectionRecord {
    pub id: String,
    pub connection: StoredConnection,
}

#[cfg(test)]
mod tests {
    use super::{ConnectionDetails, StoredConnection};

    #[test]
    fn nested_record_with_malformed_details_degrades_to_defaults() {
        // A wrong-typed field inside the nested shape must not kill the
        // record: it loads with default details and can be repaired in the
        // UI.  Regression: the strict from_value path used to fail the whole
        // connection list on such rows.
        let raw = r#"{
            "protocol":"ssh",
            "name":"broken-ish",
            "host":"10.0.0.1",
            "user":"root",
            "options":{"terminal_highlight_enabled":"true"},
            "details":{"protocol":"ssh","saved_credential_id":42}
        }"#;

        let connection: StoredConnection =
            serde_json::from_str(raw).expect("record should load with degraded fields");

        assert_eq!(connection.host, "10.0.0.1");
        // Both malformed sub-objects degraded to defaults.
        assert_eq!(connection.options.terminal_highlight_enabled, None);
        assert!(matches!(connection.details, ConnectionDetails::Ssh(_)));
        if let ConnectionDetails::Ssh(details) = connection.details {
            assert_eq!(details.saved_credential_id, None);
        }
    }

    #[test]
    fn nested_record_with_unknown_protocol_is_rejected() {
        let raw = r#"{
            "protocol":"gopher",
            "details":{"protocol":"gopher"}
        }"#;

        assert!(serde_json::from_str::<StoredConnection>(raw).is_err());
    }
}
