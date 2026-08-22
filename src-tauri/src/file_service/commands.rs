use tauri::AppHandle;

use crate::{
    file_service::{
        manager::FileServiceService,
        models::{FileServiceConfig, FileServicePublicConfig},
    },
    logging,
    paths::models::DirectoryPickerRequest,
    state::AppState,
};

fn public(config: FileServiceConfig) -> FileServicePublicConfig {
    FileServicePublicConfig::from_config(&config)
}

#[tauri::command]
pub(crate) fn get_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<FileServicePublicConfig, String> {
    logging::event("file_service.commands", "file_service.config.get").trace();
    Ok(public(FileServiceService::new(app, state.inner()).config()))
}

#[tauri::command]
pub(crate) async fn choose_shared_directory(
    app: AppHandle,
    request: DirectoryPickerRequest,
) -> Result<Option<String>, String> {
    crate::paths::commands::path_settings_choose_directory(app, request).await
}

#[tauri::command]
pub(crate) async fn start_file_service(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    protocol: String,
    bind_ip: String,
    shared_dir: String,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .start(protocol, bind_ip, shared_dir)
        .await
        .map(public)
}

#[tauri::command]
pub(crate) async fn stop(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .stop()
        .await
        .map(public)
}

#[tauri::command]
pub(crate) async fn set_bind_ip(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bind_ip: String,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .update_bind_ip(bind_ip)
        .await
        .map(public)
}

#[tauri::command]
pub(crate) async fn set_shared_dir(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    shared_dir: String,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .update_shared_dir(shared_dir)
        .await
        .map(public)
}

#[tauri::command]
pub(crate) async fn set_credentials(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    username: String,
    password: String,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .update_credentials(username, password)
        .await
        .map(public)
}

/// Stores the file service password in the OS credential vault. An empty
/// password resets the service to the built-in default password.
#[tauri::command]
pub(crate) async fn file_service_set_password(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    password: String,
) -> Result<FileServicePublicConfig, String> {
    FileServiceService::new(app, state.inner())
        .set_password(password)
        .await
        .map(public)
}
