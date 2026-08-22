mod connection_store;
mod credential_store;
mod host_key_store;
mod models;
pub(crate) mod repository;
mod schema;
mod settings;

pub use models::{
    AppPreferences, ConnectionDetails, ConnectionOptions, SerialConnectionDetails,
    SshConnectionDetails, Store, StoredConnection, StoredConnectionRecord, StoredCredential,
    StoredCredentialRecord, TelnetConnectionDetails,
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
    let mut schemes = parse_runtime_highlight_schemes(&stored_schemes)?;
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
    let imported: Vec<ExportedHighlightScheme> = serde_json::from_str(raw)
        .map_err(|error| format!("failed to parse highlight schemes JSON: {error}"))?;
    imported
        .into_iter()
        .map(|scheme| {
            let rules = scheme
                .rules
                .into_iter()
                .map(normalize_runtime_highlight_rule)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RuntimeHighlightScheme {
                id: crate::ids::new_id(),
                name: scheme.name,
                enabled: true,
                themes: Vec::new(),
                rules,
            })
        })
        .collect()
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

fn normalize_runtime_highlight_rule(
    rule: RuntimeHighlightRule,
) -> Result<RuntimeHighlightRule, String> {
    let pattern = rule.pattern.trim().to_string();
    if pattern.is_empty() {
        return Err("highlight rule pattern cannot be empty".to_string());
    }
    let match_type = match rule.match_type.as_str() {
        "text" | "regex" => rule.match_type,
        _ => return Err("highlight rule matchType must be 'text' or 'regex'".to_string()),
    };
    let effect = match rule.effect.as_str() {
        "foreground" | "background" => rule.effect,
        _ => return Err("highlight rule effect must be 'foreground' or 'background'".to_string()),
    };
    let color = rule.color.trim().to_ascii_lowercase();
    if !is_hex_color(&color) {
        return Err("highlight rule color must be a hex color like #fbbf24".to_string());
    }
    Ok(RuntimeHighlightRule {
        match_type,
        pattern,
        case_sensitive: rule.case_sensitive,
        effect,
        color,
    })
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes().iter().skip(1).all(u8::is_ascii_hexdigit)
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
