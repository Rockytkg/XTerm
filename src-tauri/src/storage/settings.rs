use std::collections::HashMap;

use redb::{ReadableDatabase, ReadableTable};

use super::schema::SETTINGS;
use super::*;

pub(super) struct SettingEntry {
    pub key: &'static str,
    pub value: String,
}

fn setting(key: &'static str, value: impl Into<String>) -> SettingEntry {
    SettingEntry {
        key,
        value: value.into(),
    }
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            enable_animations: true,
            ui_font_size: default_ui_font_size(),
            locale: default_locale(),
            show_latency: true,
            proxy_toolbar_enabled: false,
            file_service_toolbar_enabled: true,
            serial_redetect_baud_shortcut: default_serial_redetect_shortcut(),
            session_recording_shortcut: default_session_recording_shortcut(),
            terminal_theme: default_terminal_theme(),
            terminal_theme_follow_app: false,
            terminal_theme_light: default_terminal_theme_light(),
            terminal_theme_dark: default_terminal_theme_dark(),
            terminal_font_family: default_terminal_font_family(),
            terminal_font_size: default_terminal_font_size(),
            terminal_line_height: default_terminal_line_height(),
            editor_font_family: default_editor_font_family(),
            editor_font_size: default_editor_font_size(),
            editor_tab_size: default_editor_tab_size(),
            editor_line_wrapping: true,
            editor_highlight_active_line: true,
            editor_theme_mode: default_editor_theme_mode(),
            terminal_scrollback: default_terminal_scrollback(),
            terminal_cursor_blink: true,
            terminal_cursor_style: default_terminal_cursor_style(),
            terminal_cursor_inactive_style: default_terminal_cursor_inactive_style(),
            terminal_cursor_width: default_terminal_cursor_width(),
            terminal_scroll_sensitivity: default_terminal_scroll_sensitivity(),
            terminal_fast_scroll_sensitivity: default_terminal_fast_scroll_sensitivity(),
            terminal_smooth_scroll_duration: default_terminal_smooth_scroll_duration(),
            terminal_alt_click_moves_cursor: true,
            terminal_right_click_selects_word: false,
            terminal_scroll_on_user_input: true,
            terminal_scroll_on_erase_in_display: false,
            terminal_draw_bold_text_in_bright_colors: false,
            terminal_minimum_contrast_ratio: default_terminal_minimum_contrast_ratio(),
            terminal_custom_glyphs: true,
            terminal_rescale_overlapping_glyphs: false,
            terminal_mac_option_is_meta: false,
            terminal_mac_option_click_forces_selection: false,
            terminal_webgl: true,
            terminal_trzsz: true,
            transfer_drag_upload: true,
            transfer_directory_upload: true,
            transfer_max_chunk_size: default_transfer_max_chunk_size(),
            transfer_drag_init_timeout: default_transfer_drag_init_timeout(),
            terminal_type: default_terminal_type(),
            terminal_search_shortcut: default_terminal_search_shortcut(),
            open_devtools_shortcut: default_open_devtools_shortcut(),
            terminal_highlight_schemes: default_terminal_highlight_schemes(),
            theme: default_theme(),
            credential_layout_mode: default_credential_layout_mode(),
            ui_theme_light: default_ui_theme_preset(),
            ui_theme_dark: default_ui_theme_preset(),
        }
    }
}

impl Store {
    pub fn log_level(&self) -> Result<Option<String>, String> {
        self.setting_value("logLevel")
    }

    pub fn set_log_level(&self, level: &str) -> Result<(), String> {
        self.set_setting("logLevel", level)
    }

    pub fn setting_value(&self, key: &str) -> Result<Option<String>, String> {
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start settings read transaction: {error}"))?;
        let table = read_txn
            .open_table(SETTINGS)
            .map_err(|error| format!("failed to open settings table: {error}"))?;
        table
            .get(key)
            .map(|value| value.map(|guard| guard.value().to_string()))
            .map_err(|error| format!("failed to read setting '{key}': {error}"))
    }

    pub fn set_preferences(&self, p: &AppPreferences) -> Result<(), String> {
        let entries = preference_setting_entries(p);
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start preferences write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(SETTINGS)
                .map_err(|error| format!("failed to open settings table: {error}"))?;
            for entry in entries {
                table
                    .insert(entry.key, entry.value.as_str())
                    .map_err(|error| format!("failed to save setting '{}': {error}", entry.key))?;
            }
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit preferences: {error}"))
    }

    fn all_settings(&self) -> Result<HashMap<String, String>, String> {
        let read_txn = self
            .database
            .begin_read()
            .map_err(|error| format!("failed to start settings read transaction: {error}"))?;
        let table = read_txn
            .open_table(SETTINGS)
            .map_err(|error| format!("failed to open settings table: {error}"))?;
        let mut map = HashMap::new();
        for row in table
            .iter()
            .map_err(|error| format!("failed to iterate settings: {error}"))?
        {
            let (key, value) =
                row.map_err(|error| format!("failed to read setting row: {error}"))?;
            map.insert(key.value().to_string(), value.value().to_string());
        }
        Ok(map)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start settings write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(SETTINGS)
                .map_err(|error| format!("failed to open settings table: {error}"))?;
            table
                .insert(key, value)
                .map_err(|error| format!("failed to save setting '{key}': {error}"))?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit setting '{key}': {error}"))
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), String> {
        let write_txn = self
            .database
            .begin_write()
            .map_err(|error| format!("failed to start settings write transaction: {error}"))?;
        {
            let mut table = write_txn
                .open_table(SETTINGS)
                .map_err(|error| format!("failed to open settings table: {error}"))?;
            table
                .remove(key)
                .map_err(|error| format!("failed to delete setting '{key}': {error}"))?;
        }
        write_txn
            .commit()
            .map_err(|error| format!("failed to commit setting '{key}' deletion: {error}"))
    }
}

pub(super) fn initial_setting_entries() -> Vec<SettingEntry> {
    let mut entries = vec![setting("logLevel", "info")];
    entries.extend(preference_setting_entries(&AppPreferences::default()));
    entries
}

/// 偏好键的单一事实来源：(结构体字段, SettingMap 访问器, 存储/JSON 键)。
/// 读取（Store::preferences）与写回（preference_setting_entries）由同一张表
/// 生成，键名只出现一次；测试再校验此表与 AppPreferences 的 serde 键一致，
/// 防止 camelCase 分词差异（如 devtools → DevTools）让前后端键名悄悄漂移。
macro_rules! define_preference_mappings {
    ($( $field:ident : $accessor:ident = $key:literal ;)*) => {
        impl Store {
            pub fn preferences(&self) -> Result<AppPreferences, String> {
                let rows = self.all_settings()?;
                let defaults = AppPreferences::default();
                Ok(AppPreferences {
                    $( $field: rows.$accessor($key, defaults.$field), )*
                })
            }
        }

        pub(super) fn preference_setting_entries(p: &AppPreferences) -> Vec<SettingEntry> {
            vec![ $( setting($key, p.$field.to_string()), )* ]
        }
    };
}

define_preference_mappings! {
    enable_animations: bool = "enableAnimations";
    ui_font_size: integer = "uiFontSize";
    locale: text = "locale";
    show_latency: bool = "showLatency";
    proxy_toolbar_enabled: bool = "proxyToolbarEnabled";
    file_service_toolbar_enabled: bool = "fileServiceToolbarEnabled";
    serial_redetect_baud_shortcut: text = "serialRedetectBaudShortcut";
    session_recording_shortcut: text = "sessionRecordingShortcut";
    terminal_theme: text = "terminalTheme";
    terminal_theme_follow_app: bool = "terminalThemeFollowApp";
    terminal_theme_light: text = "terminalThemeLight";
    terminal_theme_dark: text = "terminalThemeDark";
    terminal_font_family: text = "terminalFontFamily";
    terminal_font_size: integer = "terminalFontSize";
    terminal_line_height: real = "terminalLineHeight";
    editor_font_family: text = "editorFontFamily";
    editor_font_size: integer = "editorFontSize";
    editor_tab_size: integer = "editorTabSize";
    editor_line_wrapping: bool = "editorLineWrapping";
    editor_highlight_active_line: bool = "editorHighlightActiveLine";
    editor_theme_mode: text = "editorThemeMode";
    terminal_scrollback: integer = "terminalScrollback";
    terminal_cursor_blink: bool = "terminalCursorBlink";
    terminal_cursor_style: text = "terminalCursorStyle";
    terminal_cursor_inactive_style: text = "terminalCursorInactiveStyle";
    terminal_cursor_width: integer = "terminalCursorWidth";
    terminal_scroll_sensitivity: real = "terminalScrollSensitivity";
    terminal_fast_scroll_sensitivity: real = "terminalFastScrollSensitivity";
    terminal_smooth_scroll_duration: integer = "terminalSmoothScrollDuration";
    terminal_alt_click_moves_cursor: bool = "terminalAltClickMovesCursor";
    terminal_right_click_selects_word: bool = "terminalRightClickSelectsWord";
    terminal_scroll_on_user_input: bool = "terminalScrollOnUserInput";
    terminal_scroll_on_erase_in_display: bool = "terminalScrollOnEraseInDisplay";
    terminal_draw_bold_text_in_bright_colors: bool = "terminalDrawBoldTextInBrightColors";
    terminal_minimum_contrast_ratio: real = "terminalMinimumContrastRatio";
    terminal_custom_glyphs: bool = "terminalCustomGlyphs";
    terminal_rescale_overlapping_glyphs: bool = "terminalRescaleOverlappingGlyphs";
    terminal_mac_option_is_meta: bool = "terminalMacOptionIsMeta";
    terminal_mac_option_click_forces_selection: bool = "terminalMacOptionClickForcesSelection";
    terminal_webgl: bool = "terminalWebgl";
    terminal_trzsz: bool = "terminalTrzsz";
    transfer_drag_upload: bool = "transferDragUpload";
    transfer_directory_upload: bool = "transferDirectoryUpload";
    transfer_max_chunk_size: integer = "transferMaxChunkSize";
    transfer_drag_init_timeout: integer = "transferDragInitTimeout";
    terminal_type: text = "terminalType";
    terminal_search_shortcut: text = "terminalSearchShortcut";
    open_devtools_shortcut: text = "openDevToolsShortcut";
    terminal_highlight_schemes: text = "terminalHighlightSchemes";
    theme: text = "theme";
    credential_layout_mode: text = "credentialLayoutMode";
    ui_theme_light: text = "uiThemeLight";
    ui_theme_dark: text = "uiThemeDark";
}

trait SettingMap {
    fn text(&self, key: &str, default: String) -> String;
    fn integer(&self, key: &str, default: i64) -> i64;
    fn real(&self, key: &str, default: f64) -> f64;
    fn bool(&self, key: &str, default: bool) -> bool;
}

/// Typed accessors over the raw settings map. A malformed stored value (e.g.
/// written by a devtools experiment or a downgraded database) falls back to
/// the default instead of failing the whole preferences read and locking the
/// user out of the settings page.
impl SettingMap for HashMap<String, String> {
    fn text(&self, key: &str, default: String) -> String {
        self.get(key).cloned().unwrap_or(default)
    }

    fn integer(&self, key: &str, default: i64) -> i64 {
        match self.get(key) {
            None => default,
            Some(v) => match v.parse() {
                Ok(parsed) => parsed,
                Err(_) => {
                    log::warn!(
                        target: "storage.settings",
                        "setting '{key}' is not a valid integer ('{v}'), using default"
                    );
                    default
                }
            },
        }
    }

    fn real(&self, key: &str, default: f64) -> f64 {
        match self.get(key) {
            None => default,
            Some(v) => match v.parse() {
                Ok(parsed) => parsed,
                Err(_) => {
                    log::warn!(
                        target: "storage.settings",
                        "setting '{key}' is not a valid number ('{v}'), using default"
                    );
                    default
                }
            },
        }
    }

    fn bool(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            None => default,
            Some(v) => match v.as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    log::warn!(
                        target: "storage.settings",
                        "setting '{key}' is not a valid boolean ('{v}'), using default"
                    );
                    default
                }
            },
        }
    }
}

pub(super) fn default_ui_font_size() -> i64 {
    14
}

pub(super) fn default_locale() -> String {
    "zh-CN".to_string()
}

pub(super) fn default_serial_redetect_shortcut() -> String {
    "Ctrl+Alt+B".to_string()
}

pub(super) fn default_session_recording_shortcut() -> String {
    "Ctrl+Alt+R".to_string()
}

pub(super) fn default_terminal_search_shortcut() -> String {
    "Ctrl+F".to_string()
}

/// No default binding: opening devtools is a power-user action and must not
/// ship with a pre-bound key. Users can still assign one in the settings UI.
pub(super) fn default_open_devtools_shortcut() -> String {
    String::new()
}

pub(super) fn default_terminal_theme() -> String {
    "default".to_string()
}

pub(super) fn default_terminal_theme_light() -> String {
    "light".to_string()
}

pub(super) fn default_terminal_theme_dark() -> String {
    "default".to_string()
}

pub(super) fn default_terminal_font_family() -> String {
    "Consolas".to_string()
}

pub(super) fn default_terminal_font_size() -> i64 {
    16
}

pub(super) fn default_terminal_line_height() -> f64 {
    1.0
}

pub(super) fn default_editor_font_family() -> String {
    default_terminal_font_family()
}

pub(super) fn default_editor_font_size() -> i64 {
    14
}

pub(super) fn default_editor_tab_size() -> i64 {
    2
}

pub(super) fn default_editor_theme_mode() -> String {
    "follow".to_string()
}

pub(super) fn default_terminal_scrollback() -> i64 {
    9001
}

pub(super) fn default_terminal_cursor_style() -> String {
    "block".to_string()
}

pub(super) fn default_terminal_cursor_inactive_style() -> String {
    "outline".to_string()
}

pub(super) fn default_terminal_cursor_width() -> i64 {
    1
}

pub(super) fn default_terminal_scroll_sensitivity() -> f64 {
    1.0
}

pub(super) fn default_terminal_fast_scroll_sensitivity() -> f64 {
    5.0
}

pub(super) fn default_terminal_smooth_scroll_duration() -> i64 {
    0
}

pub(super) fn default_terminal_minimum_contrast_ratio() -> f64 {
    1.0
}

pub(super) fn default_terminal_type() -> String {
    "xterm-256color".to_string()
}

pub(super) fn default_transfer_max_chunk_size() -> i64 {
    10 * 1024 * 1024
}

pub(super) fn default_transfer_drag_init_timeout() -> i64 {
    3000
}

pub(super) fn default_terminal_highlight_schemes() -> String {
    "[]".to_string()
}

pub(super) fn default_theme() -> String {
    "light".to_string()
}

pub(super) fn default_credential_layout_mode() -> String {
    "graph".to_string()
}

pub(super) fn default_ui_theme_preset() -> String {
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::{preference_setting_entries, SettingMap};
    use crate::storage::AppPreferences;
    use std::collections::{BTreeSet, HashMap};

    /// 存储/写回键表必须与 AppPreferences 序列化给前端的 JSON 键完全一致：
    /// serde camelCase 对 devtools、webgl 这类词的分词结果不一定等于前端约定
    /// 键名，漂移会导致"存得上、读不回"（见 open_devtools_shortcut 的 rename）。
    #[test]
    fn preference_keys_match_serialized_fields() {
        let defaults = AppPreferences::default();
        let serialized = serde_json::to_value(&defaults).expect("preferences serialize");
        let json_keys: BTreeSet<String> = serialized
            .as_object()
            .expect("preferences serialize to an object")
            .keys()
            .cloned()
            .collect();
        let entry_keys: BTreeSet<String> = preference_setting_entries(&defaults)
            .iter()
            .map(|entry| entry.key.to_string())
            .collect();
        assert_eq!(json_keys, entry_keys);
    }

    #[test]
    fn malformed_setting_values_fall_back_to_defaults() {
        let mut rows = HashMap::new();
        rows.insert("uiFontSize".to_string(), "not-a-number".to_string());
        rows.insert("terminalLineHeight".to_string(), "tall".to_string());
        rows.insert("showLatency".to_string(), "yes".to_string());

        assert_eq!(rows.integer("uiFontSize", 14), 14);
        assert_eq!(rows.real("terminalLineHeight", 1.0), 1.0);
        assert!(rows.bool("showLatency", true));
    }

    #[test]
    fn valid_setting_values_parse_through() {
        let mut rows = HashMap::new();
        rows.insert("uiFontSize".to_string(), "18".to_string());
        rows.insert("terminalLineHeight".to_string(), "1.5".to_string());
        rows.insert("showLatency".to_string(), "false".to_string());

        assert_eq!(rows.integer("uiFontSize", 14), 18);
        assert_eq!(rows.real("terminalLineHeight", 1.0), 1.5);
        assert!(!rows.bool("showLatency", true));
    }
}
