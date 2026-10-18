mod connection_store;
mod credential_store;
mod host_key_store;
mod models;
pub(crate) mod repository;
mod schema;
mod settings;

pub use models::{
    AppPreferences, ConnectionDetails, ConnectionOptions, RdpConnectionDetails,
    SerialConnectionDetails, SshConnectionDetails, Store, StoreOpenError, StoredConnection,
    StoredConnectionRecord, StoredCredential, StoredCredentialRecord, TelnetConnectionDetails,
    VncConnectionDetails,
};
pub(crate) use repository::SettingsRepository;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::{logging, state::AppState};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeHighlightScheme {
    id: String,
    name: String,
    enabled: bool,
    themes: Vec<String>,
    rules: Vec<RuntimeHighlightRule>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeHighlightRule {
    match_type: String,
    pattern: String,
    #[serde(default)]
    case_sensitive: bool,
    effect: String,
    color: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedHighlightScheme {
    name: String,
    rules: Vec<RuntimeHighlightRule>,
}

#[tauri::command]
pub(crate) async fn preferences_get(
    state: tauri::State<'_, AppState>,
) -> Result<AppPreferences, String> {
    logging::event("storage.commands", "preferences.get").trace();
    state
        .inner()
        .run_store_blocking(|store| SettingsRepository::preferences(store))
        .await
}

#[tauri::command]
pub(crate) async fn setting_set(
    state: tauri::State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    logging::event("storage.commands", "preferences.setting_set")
        .field("key", &key)
        .trace();
    state
        .inner()
        .run_store_blocking(move |store| SettingsRepository::set_setting(store, &key, &value))
        .await
}

#[tauri::command]
pub(crate) async fn setting_get(
    state: tauri::State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    state
        .inner()
        .run_store_blocking(move |store| SettingsRepository::setting_value(store, &key))
        .await
}

#[tauri::command]
pub(crate) async fn preferences_reset(
    state: tauri::State<'_, AppState>,
) -> Result<AppPreferences, String> {
    crate::paths::commands::reset_path_settings(state.inner()).await?;
    let preferences = AppPreferences::default();
    let saved_preferences = preferences.clone();
    state
        .inner()
        .run_store_blocking(move |store| {
            SettingsRepository::set_preferences(store, &saved_preferences)
        })
        .await?;
    logging::event("storage.commands", "preferences.reset").warn();
    Ok(preferences)
}

#[tauri::command]
pub(crate) async fn terminal_highlight_schemes_import(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<Vec<RuntimeHighlightScheme>>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Import keyword highlight schemes")
        .add_filter("JSON files", &["json"])
        .add_filter("All files", &["*"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver.await.map_err(|_| {
        "highlight scheme import picker closed before returning a result".to_string()
    })?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve selected file: {error}"))?;
    let raw = tokio::fs::read_to_string(&path).await.map_err(|error| {
        format!(
            "failed to read highlight schemes file '{}': {error}",
            path.to_string_lossy()
        )
    })?;
    let stored_schemes = state
        .inner()
        .run_store_blocking(|store| {
            SettingsRepository::preferences(store).map(|value| value.terminal_highlight_schemes)
        })
        .await?;
    let mut schemes = parse_runtime_highlight_schemes(&stored_schemes).unwrap_or_else(|error| {
        // A corrupt stored list must not block importing new schemes; the
        // unreadable entries were already invisible to the UI.
        logging::event(
            "storage.commands",
            "terminal_highlight_schemes.stored_parse_failed",
        )
        .field("error", &error)
        .warn();
        Vec::new()
    });
    let imported = parse_imported_highlight_schemes(&raw)?;
    schemes.extend(imported);
    let serialized = serde_json::to_string(&schemes)
        .map_err(|error| format!("failed to serialize highlight schemes: {error}"))?;
    state
        .inner()
        .run_store_blocking(move |store| {
            SettingsRepository::set_setting(store, "terminalHighlightSchemes", &serialized)
        })
        .await?;
    logging::event("storage.commands", "terminal_highlight_schemes.import")
        .field("path", path.to_string_lossy())
        .field("count", schemes.len())
        .info();
    Ok(Some(schemes))
}

#[tauri::command]
pub(crate) async fn terminal_highlight_schemes_export(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    scheme_id: String,
) -> Result<Option<String>, String> {
    let stored_schemes = state
        .inner()
        .run_store_blocking(|store| {
            SettingsRepository::preferences(store).map(|value| value.terminal_highlight_schemes)
        })
        .await?;
    let schemes = parse_stored_highlight_schemes(&stored_schemes, &scheme_id)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Export keyword highlight schemes")
        .set_file_name("keyword-highlight-schemes.json")
        .add_filter("JSON files", &["json"])
        .add_filter("All files", &["*"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver.await.map_err(|_| {
        "highlight scheme export picker closed before returning a result".to_string()
    })?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve export path: {error}"))?;
    let path = ensure_json_extension(&path);
    let raw = serde_json::to_string_pretty(&schemes)
        .map_err(|error| format!("failed to serialize highlight schemes: {error}"))?;
    tokio::fs::write(&path, raw).await.map_err(|error| {
        format!(
            "failed to write highlight schemes file '{}': {error}",
            path.to_string_lossy()
        )
    })?;
    logging::event("storage.commands", "terminal_highlight_schemes.export")
        .field("scheme_id", &scheme_id)
        .field("path", path.to_string_lossy())
        .info();
    Ok(Some(path.to_string_lossy().to_string()))
}

fn parse_imported_highlight_schemes(raw: &str) -> Result<Vec<RuntimeHighlightScheme>, String> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| format!("failed to parse highlight schemes JSON: {error}"))?;
    // Accept both an array of schemes and a single scheme object.
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(_) => vec![value],
        _ => {
            return Err(
                "highlight schemes file must contain a scheme object or an array".to_string(),
            )
        }
    };
    // Import is Postel-style at rule granularity: an invalid rule or scheme
    // is skipped (with a log line) instead of failing the whole file — the
    // runtime highlighter ignores invalid rules in exactly the same way.
    let mut imported = Vec::new();
    let mut skipped_schemes = 0usize;
    let mut skipped_rules = 0usize;
    for item in items {
        let Ok(scheme) = serde_json::from_value::<ImportedHighlightScheme>(item) else {
            skipped_schemes += 1;
            continue;
        };
        let mut rules = Vec::with_capacity(scheme.rules.len());
        for rule in scheme.rules {
            match normalize_imported_highlight_rule(rule) {
                Ok(rule) => rules.push(rule),
                Err(reason) => {
                    skipped_rules += 1;
                    logging::event(
                        "storage.commands",
                        "terminal_highlight_schemes.rule_skipped",
                    )
                    .field("reason", &reason)
                    .debug();
                }
            }
        }
        if rules.is_empty() {
            skipped_schemes += 1;
            continue;
        }
        imported.push(RuntimeHighlightScheme {
            id: crate::ids::new_id(),
            name: scheme.name,
            enabled: true,
            themes: Vec::new(),
            rules,
        });
    }
    if skipped_schemes > 0 || skipped_rules > 0 {
        logging::event(
            "storage.commands",
            "terminal_highlight_schemes.import_skipped",
        )
        .field("schemes", skipped_schemes)
        .field("rules", skipped_rules)
        .warn();
    }
    if imported.is_empty() {
        return Err("no valid highlight schemes found in the file".to_string());
    }
    Ok(imported)
}

/// Import-side scheme shape: missing names get a fallback, and rules stay
/// raw so a single malformed rule cannot take down its whole scheme.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportedHighlightScheme {
    #[serde(default = "default_imported_scheme_name")]
    name: String,
    #[serde(default)]
    rules: Vec<serde_json::Value>,
}

fn default_imported_scheme_name() -> String {
    "Imported".to_string()
}

/// Import-side rule shape: only the semantically load-bearing fields are
/// required; `case_sensitive` defaults to false.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportedHighlightRule {
    match_type: String,
    pattern: String,
    #[serde(default)]
    case_sensitive: bool,
    effect: String,
    color: String,
}

fn normalize_imported_highlight_rule(
    rule: serde_json::Value,
) -> Result<RuntimeHighlightRule, String> {
    let rule: ImportedHighlightRule =
        serde_json::from_value(rule).map_err(|error| error.to_string())?;
    let pattern = rule.pattern.trim().to_string();
    if pattern.is_empty() {
        return Err("empty pattern".to_string());
    }
    let match_type = rule.match_type.trim().to_ascii_lowercase();
    if match_type != "text" && match_type != "regex" {
        return Err(format!("unknown matchType '{}'", rule.match_type));
    }
    let effect = rule.effect.trim().to_ascii_lowercase();
    if effect != "foreground" && effect != "background" {
        return Err(format!("unknown effect '{}'", rule.effect));
    }
    let color = normalize_hex_color(&rule.color)
        .ok_or_else(|| format!("invalid color '{}'", rule.color))?;
    Ok(RuntimeHighlightRule {
        match_type,
        pattern,
        case_sensitive: rule.case_sensitive,
        effect,
        color,
    })
}

/// Accepts `#rgb`, `#rgba`, `#rrggbb` and `#rrggbbaa` in any casing (the `#`
/// prefix is optional) and normalizes to lowercase `#rrggbb`/`#rrggbbaa`.
fn normalize_hex_color(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let full: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|digit| [digit, digit]).collect(),
        6 | 8 => hex.to_string(),
        _ => return None,
    };
    Some(format!("#{full}").to_ascii_lowercase())
}

fn parse_runtime_highlight_schemes(raw: &str) -> Result<Vec<RuntimeHighlightScheme>, String> {
    serde_json::from_str(raw)
        .map_err(|error| format!("failed to parse stored highlight schemes JSON: {error}"))
}

fn parse_stored_highlight_schemes(
    raw: &str,
    scheme_id: &str,
) -> Result<Vec<ExportedHighlightScheme>, String> {
    let stored = parse_runtime_highlight_schemes(raw)?;
    let selected: Vec<ExportedHighlightScheme> = stored
        .into_iter()
        .filter(|scheme| scheme.id == scheme_id)
        .map(|scheme| ExportedHighlightScheme {
            name: scheme.name,
            rules: scheme.rules,
        })
        .collect();
    if selected.is_empty() {
        Err("selected highlight scheme was not found".to_string())
    } else {
        Ok(selected)
    }
}

fn ensure_json_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
    {
        path.to_path_buf()
    } else {
        path.with_extension("json")
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_hex_color, parse_imported_highlight_schemes};

    #[test]
    fn import_skips_bad_rules_and_schemes_instead_of_failing() {
        // One valid rule, one with an unknown matchType, one with a bad
        // color, one empty-pattern draft, plus a scheme with no valid rules.
        let raw = r##"[
            {
                "name": "mixed",
                "rules": [
                    {"matchType": "regex", "pattern": "ERROR", "effect": "foreground", "color": "#f55"},
                    {"matchType": "fuzzy", "pattern": "WARN", "effect": "foreground", "color": "#ffae42"},
                    {"matchType": "text", "pattern": "INFO", "effect": "foreground", "color": "blue"},
                    {"matchType": "text", "pattern": "   ", "effect": "background", "color": "#ffffff"}
                ]
            },
            {"name": "empty", "rules": []},
            "not a scheme"
        ]"##;
        let schemes = parse_imported_highlight_schemes(raw).unwrap();
        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0].name, "mixed");
        assert_eq!(schemes[0].rules.len(), 1);
        let rule = &schemes[0].rules[0];
        assert_eq!(rule.pattern, "ERROR");
        assert_eq!(rule.color, "#ff5555"); // #rgb expanded
        assert!(!rule.case_sensitive); // missing field defaults to false

        // A file with no valid rules at all still fails honestly.
        assert!(parse_imported_highlight_schemes(r#"[{"name":"x","rules":[]}]"#).is_err());
    }

    #[test]
    fn import_accepts_a_single_scheme_object_and_lax_field_spelling() {
        let raw = r#"{
            "name": "single",
            "rules": [
                {"matchType": "TEXT", "pattern": "OK", "effect": "Foreground", "color": "00FF00"}
            ]
        }"#;
        let schemes = parse_imported_highlight_schemes(raw).unwrap();
        assert_eq!(schemes.len(), 1);
        let rule = &schemes[0].rules[0];
        assert_eq!(rule.match_type, "text");
        assert_eq!(rule.effect, "foreground");
        assert_eq!(rule.color, "#00ff00");
    }

    #[test]
    fn hex_color_normalization_accepts_css_forms() {
        assert_eq!(normalize_hex_color("#f55").as_deref(), Some("#ff5555"));
        assert_eq!(normalize_hex_color("#f558").as_deref(), Some("#ff555588"));
        assert_eq!(normalize_hex_color("FF5555").as_deref(), Some("#ff5555"));
        assert_eq!(
            normalize_hex_color("#FF555580").as_deref(),
            Some("#ff555580")
        );
        assert_eq!(
            normalize_hex_color("#ff5555ff").as_deref(),
            Some("#ff5555ff")
        );
        assert_eq!(normalize_hex_color("blue"), None);
        assert_eq!(normalize_hex_color("#ff"), None);
    }
}
