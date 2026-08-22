use std::path::Path;

use redb::Database;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};

use super::schema::initialize_schema;

pub(super) const DATABASE_FILE_NAME: &str = "data.redb";

pub struct Store {
    pub(super) database: Database,
}

impl Store {
    pub fn open(database_dir: &Path) -> Result<Self, String> {
        let database_path = database_dir.join(DATABASE_FILE_NAME);
        let database = Database::create(&database_path)
            .map_err(|e| format!("failed to open Redb store: {e}"))?;
        initialize_schema(&database)?;
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
}

/// Protocol-specific details persisted for a connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "protocol", rename_all = "camelCase")]
pub enum ConnectionDetails {
    Ssh(SshConnectionDetails),
    Telnet(TelnetConnectionDetails),
    Serial(SerialConnectionDetails),
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

/// Persisted connection record.
///
/// New records are written in a nested shape with `options` and `details`.
/// A custom deserializer still accepts legacy flat records where the same
/// fields lived at the top level.
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

        let (options, details) = if obj.contains_key("details") || obj.contains_key("options") {
            let options_value = obj.remove("options").unwrap_or_default();
            let options: ConnectionOptions =
                serde_json::from_value(options_value).map_err(D::Error::custom)?;
            let details_value = obj
                .remove("details")
                .ok_or_else(|| D::Error::custom("missing details field"))?;
            let details: ConnectionDetails =
                serde_json::from_value(details_value).map_err(D::Error::custom)?;
            (options, details)
        } else {
            let options = ConnectionOptions {
                terminal_type: take_string(obj, "terminal_type")
                    .or_else(|| take_string(obj, "terminalType")),
                encoding: take_string(obj, "encoding"),
                backspace_sends: take_string(obj, "backspace_sends")
                    .or_else(|| take_string(obj, "backspaceSends")),
                realtime_encoding_detection: take_bool(obj, "realtime_encoding_detection")
                    .or_else(|| take_bool(obj, "realtimeEncodingDetection")),
                terminal_highlight_enabled: take_bool(obj, "terminal_highlight_enabled")
                    .or_else(|| take_bool(obj, "terminalHighlightEnabled")),
                terminal_more_prompt_cleanup: take_bool(obj, "terminal_more_prompt_cleanup")
                    .or_else(|| take_bool(obj, "terminalMorePromptCleanup")),
            };

            let details = match protocol.as_str() {
                "ssh" => ConnectionDetails::Ssh(SshConnectionDetails {
                    auth_method: take_string(obj, "auth_method")
                        .or_else(|| take_string(obj, "authMethod")),
                    saved_credential_id: take_string(obj, "saved_credential_id")
                        .or_else(|| take_string(obj, "savedCredentialId")),
                    jump_hosts: take_string(obj, "jump_hosts")
                        .or_else(|| take_string(obj, "jumpHosts")),
                }),
                "telnet" => ConnectionDetails::Telnet(TelnetConnectionDetails {
                    auth_method: take_string(obj, "auth_method")
                        .or_else(|| take_string(obj, "authMethod")),
                    saved_credential_id: take_string(obj, "saved_credential_id")
                        .or_else(|| take_string(obj, "savedCredentialId")),
                }),
                "serial" => ConnectionDetails::Serial(SerialConnectionDetails {
                    auth_method: take_string(obj, "auth_method")
                        .or_else(|| take_string(obj, "authMethod")),
                    saved_credential_id: take_string(obj, "saved_credential_id")
                        .or_else(|| take_string(obj, "savedCredentialId")),
                    baud_rate: take_i64(obj, "baud_rate").or_else(|| take_i64(obj, "baudRate")),
                    serial_quick_auto_baud: take_bool(obj, "serial_quick_auto_baud")
                        .or_else(|| take_bool(obj, "serialQuickAutoBaud")),
                    data_bits: take_i64(obj, "data_bits").or_else(|| take_i64(obj, "dataBits")),
                    flow_control: take_string(obj, "flow_control")
                        .or_else(|| take_string(obj, "flowControl")),
                    parity: take_string(obj, "parity"),
                    stop_bits: take_i64(obj, "stop_bits").or_else(|| take_i64(obj, "stopBits")),
                }),
                _ => {
                    return Err(D::Error::custom(format!(
                        "unsupported connection protocol '{protocol}'"
                    )))
                }
            };
            (options, details)
        };

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

fn take_string(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    obj.remove(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_str().map(str::to_string)
        }
    })
}

fn take_bool(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<bool> {
    obj.remove(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_bool()
        }
    })
}

fn take_i64(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<i64> {
    obj.remove(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_i64()
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
    fn stored_connection_accepts_legacy_records_without_more_prompt_cleanup() {
        let raw = r#"{
            "protocol":"telnet",
            "port":"23",
            "name":"legacy",
            "host":"10.0.0.1",
            "user":"",
            "baudRate":null,
            "authMethod":"password",
            "savedCredentialId":null,
            "terminalHighlightEnabled":true,
            "serialQuickAutoBaud":null,
            "dataBits":null,
            "flowControl":null,
            "parity":null,
            "stopBits":null,
            "jumpHosts":null,
            "terminalType":"vt100",
            "encoding":null,
            "backspaceSends":null
        }"#;

        let connection: StoredConnection =
            serde_json::from_str(raw).expect("legacy connection should deserialize");

        assert_eq!(connection.options.terminal_more_prompt_cleanup, None);
        assert_eq!(connection.options.terminal_type, Some("vt100".to_string()));
        assert_eq!(connection.options.terminal_highlight_enabled, Some(true));
        match connection.details {
            ConnectionDetails::Telnet(details) => {
                assert_eq!(details.auth_method, Some("password".to_string()));
                assert_eq!(details.saved_credential_id, None);
            }
            _ => panic!("expected telnet details"),
        }
    }

    #[test]
    fn stored_credential_accepts_legacy_username_field() {
        let raw = r#"{
            "position":1,
            "cred_type":"password",
            "name":"legacy password",
            "username":"admin",
            "password":"secret",
            "private_key":null,
            "passphrase":null
        }"#;

        let credential: super::StoredCredential =
            serde_json::from_str(raw).expect("legacy credential should deserialize");

        assert_eq!(credential.name, "legacy password");
        assert_eq!(credential.cred_type, "password");
    }
}
