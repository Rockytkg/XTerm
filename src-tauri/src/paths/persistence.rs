use std::path::{Path, PathBuf};

use super::{PersistedPathSettings, DATA_DIR_NAME};

pub(super) fn read_path_settings(install_dir: &Path) -> Result<PersistedPathSettings, String> {
    let path = path_settings_file(install_dir);
    if !path.exists() {
        return Ok(empty_path_settings());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read path settings: {error}"))?;
    serde_json::from_str(&raw).map_err(|error| format!("failed to parse path settings: {error}"))
}

pub(super) async fn write_path_settings_async(
    install_dir: &Path,
    settings: &PersistedPathSettings,
) -> Result<(), String> {
    let path = path_settings_file(install_dir);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("failed to create path settings directory: {error}"))?;
    }
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("failed to serialize path settings: {error}"))?;
    tokio::fs::write(path, raw)
        .await
        .map_err(|error| format!("failed to write path settings: {error}"))
}

fn path_settings_file(install_dir: &Path) -> PathBuf {
    install_dir.join(DATA_DIR_NAME).join("paths.json")
}

fn empty_path_settings() -> PersistedPathSettings {
    PersistedPathSettings {
        data_dir: None,
        log_dir: None,
    }
}
