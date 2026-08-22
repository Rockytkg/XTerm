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

    pub fn preferences(&self) -> Result<AppPreferences, String> {
        let rows = self.all_settings()?;
        let defaults = AppPreferences::default();
        Ok(AppPreferences {
            enable_animations: rows.bool("enableAnimations", defaults.enable_animations),
            ui_font_size: rows.integer("uiFontSize", defaults.ui_font_size),
            locale: rows.text("locale", defaults.locale),
            show_latency: rows.bool("showLatency", defaults.show_latency),
            proxy_toolbar_enabled: rows.bool("proxyToolbarEnabled", defaults.proxy_toolbar_enabled),
            file_service_toolbar_enabled: rows.bool(
                "fileServiceToolbarEnabled",
                defaults.file_service_toolbar_enabled,
            ),
            serial_redetect_baud_shortcut: rows.text(
                "serialRedetectBaudShortcut",
                defaults.serial_redetect_baud_shortcut,
            ),
            session_recording_shortcut: rows.text(
                "sessionRecordingShortcut",
                defaults.session_recording_shortcut,
            ),
            terminal_theme: rows.text("terminalTheme", defaults.terminal_theme),
            terminal_theme_follow_app: rows
                .bool("terminalThemeFollowApp", defaults.terminal_theme_follow_app),
            terminal_theme_light: rows.text("terminalThemeLight", defaults.terminal_theme_light),
            terminal_theme_dark: rows.text("terminalThemeDark", defaults.terminal_theme_dark),
            terminal_font_family: rows.text("terminalFontFamily", defaults.terminal_font_family),
            terminal_font_size: rows.integer("terminalFontSize", defaults.terminal_font_size),
            terminal_line_height: rows.real("terminalLineHeight", defaults.terminal_line_height),
            editor_font_family: rows.text("editorFontFamily", defaults.editor_font_family),
            editor_font_size: rows.integer("editorFontSize", defaults.editor_font_size),
            editor_tab_size: rows.integer("editorTabSize", defaults.editor_tab_size),
            editor_line_wrapping: rows.bool("editorLineWrapping", defaults.editor_line_wrapping),
            editor_highlight_active_line: rows.bool(
                "editorHighlightActiveLine",
                defaults.editor_highlight_active_line,
            ),
            editor_theme_mode: rows.text("editorThemeMode", defaults.editor_theme_mode),
            terminal_scrollback: rows.integer("terminalScrollback", defaults.terminal_scrollback),
            terminal_cursor_blink: rows.bool("terminalCursorBlink", defaults.terminal_cursor_blink),
            terminal_cursor_style: rows.text("terminalCursorStyle", defaults.terminal_cursor_style),
            terminal_cursor_inactive_style: rows.text(
                "terminalCursorInactiveStyle",
                defaults.terminal_cursor_inactive_style,
            ),
            terminal_cursor_width: rows
                .integer("terminalCursorWidth", defaults.terminal_cursor_width),
            terminal_scroll_sensitivity: rows.real(
                "terminalScrollSensitivity",
                defaults.terminal_scroll_sensitivity,
            ),
            terminal_fast_scroll_sensitivity: rows.real(
                "terminalFastScrollSensitivity",
                defaults.terminal_fast_scroll_sensitivity,
            ),
            terminal_smooth_scroll_duration: rows.integer(
                "terminalSmoothScrollDuration",
                defaults.terminal_smooth_scroll_duration,
            ),
            terminal_alt_click_moves_cursor: rows.bool(
                "terminalAltClickMovesCursor",
                defaults.terminal_alt_click_moves_cursor,
            ),
            terminal_right_click_selects_word: rows.bool(
                "terminalRightClickSelectsWord",
                defaults.terminal_right_click_selects_word,
            ),
            terminal_scroll_on_user_input: rows.bool(
                "terminalScrollOnUserInput",
                defaults.terminal_scroll_on_user_input,
            ),
            terminal_scroll_on_erase_in_display: rows.bool(
                "terminalScrollOnEraseInDisplay",
                defaults.terminal_scroll_on_erase_in_display,
            ),
            terminal_draw_bold_text_in_bright_colors: rows.bool(
                "terminalDrawBoldTextInBrightColors",
                defaults.terminal_draw_bold_text_in_bright_colors,
            ),
            terminal_minimum_contrast_ratio: rows.real(
                "terminalMinimumContrastRatio",
                defaults.terminal_minimum_contrast_ratio,
            ),
            terminal_custom_glyphs: rows
                .bool("terminalCustomGlyphs", defaults.terminal_custom_glyphs),
            terminal_rescale_overlapping_glyphs: rows.bool(
                "terminalRescaleOverlappingGlyphs",
                defaults.terminal_rescale_overlapping_glyphs,
            ),
            terminal_mac_option_is_meta: rows.bool(
                "terminalMacOptionIsMeta",
                defaults.terminal_mac_option_is_meta,
            ),
            terminal_mac_option_click_forces_selection: rows.bool(
                "terminalMacOptionClickForcesSelection",
                defaults.terminal_mac_option_click_forces_selection,
            ),
            terminal_webgl: rows.bool("terminalWebgl", defaults.terminal_webgl),
            terminal_trzsz: rows.bool("terminalTrzsz", defaults.terminal_trzsz),
            transfer_drag_upload: rows.bool("transferDragUpload", defaults.transfer_drag_upload),
            transfer_directory_upload: rows.bool(
                "transferDirectoryUpload",
                defaults.transfer_directory_upload,
            ),
            transfer_max_chunk_size: rows
                .integer("transferMaxChunkSize", defaults.transfer_max_chunk_size),
            transfer_drag_init_timeout: rows.integer(
                "transferDragInitTimeout",
                defaults.transfer_drag_init_timeout,
            ),
            terminal_type: rows.text("terminalType", defaults.terminal_type),
            terminal_search_shortcut: rows
                .text("terminalSearchShortcut", defaults.terminal_search_shortcut),
            open_devtools_shortcut: rows
                .text("openDevToolsShortcut", defaults.open_devtools_shortcut),
            terminal_highlight_schemes: rows.text(
                "terminalHighlightSchemes",
                defaults.terminal_highlight_schemes,
            ),
            theme: rows.text("theme", defaults.theme),
            credential_layout_mode: rows
                .text("credentialLayoutMode", defaults.credential_layout_mode),
            ui_theme_light: rows.text("uiThemeLight", defaults.ui_theme_light),
            ui_theme_dark: rows.text("uiThemeDark", defaults.ui_theme_dark),
        })
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

pub(super) fn preference_setting_entries(p: &AppPreferences) -> Vec<SettingEntry> {
    vec![
        setting("enableAnimations", p.enable_animations.to_string()),
        setting("uiFontSize", p.ui_font_size.to_string()),
        setting("locale", p.locale.clone()),
        setting("showLatency", p.show_latency.to_string()),
        setting("proxyToolbarEnabled", p.proxy_toolbar_enabled.to_string()),
        setting(
            "fileServiceToolbarEnabled",
            p.file_service_toolbar_enabled.to_string(),
        ),
        setting(
            "serialRedetectBaudShortcut",
            p.serial_redetect_baud_shortcut.clone(),
        ),
        setting(
            "sessionRecordingShortcut",
            p.session_recording_shortcut.clone(),
        ),
        setting("terminalTheme", p.terminal_theme.clone()),
        setting(
            "terminalThemeFollowApp",
            p.terminal_theme_follow_app.to_string(),
        ),
        setting("terminalThemeLight", p.terminal_theme_light.clone()),
        setting("terminalThemeDark", p.terminal_theme_dark.clone()),
        setting("terminalFontFamily", p.terminal_font_family.clone()),
        setting("terminalFontSize", p.terminal_font_size.to_string()),
        setting("terminalLineHeight", p.terminal_line_height.to_string()),
        setting("editorFontFamily", p.editor_font_family.clone()),
        setting("editorFontSize", p.editor_font_size.to_string()),
        setting("editorTabSize", p.editor_tab_size.to_string()),
        setting("editorLineWrapping", p.editor_line_wrapping.to_string()),
        setting(
            "editorHighlightActiveLine",
            p.editor_highlight_active_line.to_string(),
        ),
        setting("editorThemeMode", p.editor_theme_mode.clone()),
        setting("terminalScrollback", p.terminal_scrollback.to_string()),
        setting("terminalCursorBlink", p.terminal_cursor_blink.to_string()),
        setting("terminalCursorStyle", p.terminal_cursor_style.clone()),
        setting(
            "terminalCursorInactiveStyle",
            p.terminal_cursor_inactive_style.clone(),
        ),
        setting("terminalCursorWidth", p.terminal_cursor_width.to_string()),
        setting(
            "terminalScrollSensitivity",
            p.terminal_scroll_sensitivity.to_string(),
        ),
        setting(
            "terminalFastScrollSensitivity",
            p.terminal_fast_scroll_sensitivity.to_string(),
        ),
        setting(
            "terminalSmoothScrollDuration",
            p.terminal_smooth_scroll_duration.to_string(),
        ),
        setting(
            "terminalAltClickMovesCursor",
            p.terminal_alt_click_moves_cursor.to_string(),
        ),
        setting(
            "terminalRightClickSelectsWord",
            p.terminal_right_click_selects_word.to_string(),
        ),
        setting(
            "terminalScrollOnUserInput",
            p.terminal_scroll_on_user_input.to_string(),
        ),
        setting(
            "terminalScrollOnEraseInDisplay",
            p.terminal_scroll_on_erase_in_display.to_string(),
        ),
        setting(
            "terminalDrawBoldTextInBrightColors",
            p.terminal_draw_bold_text_in_bright_colors.to_string(),
        ),
        setting(
            "terminalMinimumContrastRatio",
            p.terminal_minimum_contrast_ratio.to_string(),
        ),
        setting("terminalCustomGlyphs", p.terminal_custom_glyphs.to_string()),
        setting(
            "terminalRescaleOverlappingGlyphs",
            p.terminal_rescale_overlapping_glyphs.to_string(),
        ),
        setting(
            "terminalMacOptionIsMeta",
            p.terminal_mac_option_is_meta.to_string(),
        ),
        setting(
            "terminalMacOptionClickForcesSelection",
            p.terminal_mac_option_click_forces_selection.to_string(),
        ),
        setting("terminalWebgl", p.terminal_webgl.to_string()),
        setting("terminalTrzsz", p.terminal_trzsz.to_string()),
        setting("transferDragUpload", p.transfer_drag_upload.to_string()),
        setting(
            "transferDirectoryUpload",
            p.transfer_directory_upload.to_string(),
        ),
        setting(
            "transferMaxChunkSize",
            p.transfer_max_chunk_size.to_string(),
        ),
        setting(
            "transferDragInitTimeout",
            p.transfer_drag_init_timeout.to_string(),
        ),
        setting("terminalType", p.terminal_type.clone()),
        setting("terminalSearchShortcut", p.terminal_search_shortcut.clone()),
        setting("openDevToolsShortcut", p.open_devtools_shortcut.clone()),
        setting(
            "terminalHighlightSchemes",
            p.terminal_highlight_schemes.clone(),
        ),
        setting("theme", p.theme.clone()),
        setting("credentialLayoutMode", p.credential_layout_mode.clone()),
        setting("uiThemeLight", p.ui_theme_light.clone()),
        setting("uiThemeDark", p.ui_theme_dark.clone()),
    ]
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
    use super::SettingMap;
    use std::collections::HashMap;

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
