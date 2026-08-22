use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::{
    file_ops::normalize_configured_path, models::DirectoryPickerRequest, AppPaths, PathSettings,
    DATABASE_DIR_NAME, DATA_DIR_NAME, LOG_DIR_NAME,
};
use crate::{logging, state::AppState};

pub async fn reset_path_settings(state: &AppState) -> Result<PathSettings, String> {
    let install_dir = state.paths().install_dir.clone();
    let base_dir = state.paths().base_dir.clone();
    let base_data_dir = base_dir.join(DATA_DIR_NAME);
    let data_dir = base_data_dir.join(DATABASE_DIR_NAME);
    let log_dir = base_data_dir.join(LOG_DIR_NAME);
    tokio::fs::create_dir_all(&base_data_dir)
        .await
        .map_err(|error| format!("failed to create app data directory: {error}"))?;
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|error| format!("failed to create database directory: {error}"))?;
    tokio::fs::create_dir_all(&log_dir)
        .await
        .map_err(|error| format!("failed to create log directory: {error}"))?;

    let next_paths = AppPaths {
        install_dir,
        base_dir,
        data_dir,
        log_dir,
    };
    next_paths.persist_settings_async().await?;
    let mut paths = state.paths();
    *paths = next_paths.clone();
    Ok(next_paths.settings())
}

/// Returns the active database and log directories used by Rust-owned output.
#[tauri::command]
pub(crate) fn path_settings_get(state: tauri::State<'_, AppState>) -> Result<PathSettings, String> {
    logging::event("paths.commands", "path_settings.get").trace();
    Ok(state.paths().settings())
}

/// Saves the next Rust-owned data and log directories. The running local store
/// and file logger keep using their startup paths until restart.
#[tauri::command]
pub(crate) async fn path_settings_set(
    state: tauri::State<'_, AppState>,
    settings: PathSettings,
) -> Result<PathSettings, String> {
    logging::event("paths.commands", "path_settings.set.start")
        .field("data_dir", &settings.data_dir)
        .field("log_dir", &settings.log_dir)
        .info();
    let next_data_dir = normalize_configured_path(&settings.data_dir, "database directory")?;
    let next_log_dir = normalize_configured_path(&settings.log_dir, "log directory")?;
    let install_dir = state.paths().install_dir.clone();
    let base_dir = state.paths().base_dir.clone();

    tokio::fs::create_dir_all(&next_data_dir)
        .await
        .map_err(|error| format!("failed to create database directory: {error}"))?;
    tokio::fs::create_dir_all(&next_log_dir)
        .await
        .map_err(|error| format!("failed to create log directory: {error}"))?;

    let next_paths = AppPaths {
        install_dir,
        base_dir,
        data_dir: next_data_dir,
        log_dir: next_log_dir,
    };
    next_paths.persist_settings_async().await?;
    let mut paths = state.paths();
    *paths = next_paths.clone();
    logging::event("paths.commands", "path_settings.set.success")
        .field("data_dir", next_paths.data_dir.to_string_lossy())
        .field("log_dir", next_paths.log_dir.to_string_lossy())
        .field("restart_required", true)
        .info();
    Ok(next_paths.settings())
}

/// Opens a native directory picker and returns the selected directory path.
#[tauri::command]
pub(crate) async fn path_settings_choose_directory(
    app: AppHandle,
    request: DirectoryPickerRequest,
) -> Result<Option<String>, String> {
    logging::event("paths.commands", "path_settings.choose_directory")
        .maybe_field("default_path", request.default_path.clone())
        .maybe_field("title", request.title.clone())
        .debug();
    let mut dialog = app.dialog().file();
    if let Some(title) = request.title.filter(|value| !value.trim().is_empty()) {
        dialog = dialog.set_title(title);
    }
    if let Some(default_path) = request
        .default_path
        .filter(|value| !value.trim().is_empty())
    {
        dialog = dialog.set_directory(default_path);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |path| {
        let _ = sender.send(path);
    });
    receiver
        .await
        .map_err(|_| "directory picker closed before returning a result".to_string())?
        .map(|path| {
            path.into_path()
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| format!("failed to resolve selected directory: {error}"))
        })
        .transpose()
}
