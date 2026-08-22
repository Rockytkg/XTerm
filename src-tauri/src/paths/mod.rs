use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) mod commands;
mod file_ops;
pub(crate) mod models;
mod persistence;

pub use models::{AppPaths, PathSettings};

use models::PersistedPathSettings;
use persistence::{read_path_settings, write_path_settings_async};

pub(super) const DATA_DIR_NAME: &str = "data";
pub(super) const DATABASE_DIR_NAME: &str = "database";
pub(super) const LOG_DIR_NAME: &str = "logs";

/// Tauri identifier from `tauri.conf.json`; names the per-user data folder
/// used when the executable directory cannot hold app data.
const APP_IDENTIFIER: &str = "com.liushicong.xterm";

impl AppPaths {
    pub fn initialize() -> Result<Self, String> {
        let install_dir = resolve_install_dir()?;
        let base_dir = base_dir_for(&install_dir);
        let base_data_dir = base_dir.join(DATA_DIR_NAME);
        let persisted = read_path_settings(&base_dir)?;
        let default_database_dir = base_data_dir.join(DATABASE_DIR_NAME);
        let default_log_dir = base_data_dir.join(LOG_DIR_NAME);
        let data_dir = persisted
            .data_dir
            .clone()
            .unwrap_or_else(|| default_database_dir.clone());
        let log_dir = persisted
            .log_dir
            .clone()
            .unwrap_or_else(|| default_log_dir.clone());

        fs::create_dir_all(&base_data_dir)
            .map_err(|error| format!("failed to create app data directory: {error}"))?;
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("failed to create database directory: {error}"))?;
        fs::create_dir_all(&log_dir)
            .map_err(|error| format!("failed to create log directory: {error}"))?;

        Ok(Self {
            install_dir,
            base_dir,
            data_dir,
            log_dir,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    pub fn settings(&self) -> PathSettings {
        PathSettings {
            install_dir: self.install_dir.to_string_lossy().to_string(),
            data_dir: self.data_dir.to_string_lossy().to_string(),
            log_dir: self.log_dir.to_string_lossy().to_string(),
        }
    }

    pub(super) async fn persist_settings_async(&self) -> Result<(), String> {
        let settings = PersistedPathSettings {
            data_dir: Some(self.data_dir.clone()),
            log_dir: Some(self.log_dir.clone()),
        };
        write_path_settings_async(&self.base_dir, &settings).await
    }
}

fn resolve_install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve executable path: {error}"))?;
    let exe_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve executable directory".to_string())?;
    Ok(install_dir_from_exe_dir(exe_dir))
}

fn install_dir_from_exe_dir(exe_dir: PathBuf) -> PathBuf {
    if exe_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "debug" || name == "release")
    {
        if let Some(target_dir) = exe_dir.parent() {
            if target_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "target")
            {
                if let Some(src_tauri_dir) = target_dir.parent() {
                    if src_tauri_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == "src-tauri")
                    {
                        return src_tauri_dir
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| src_tauri_dir.to_path_buf());
                    }
                }
            }
        }
    }
    exe_dir
}

/// Directory that anchors `data/` (database, logs, `paths.json`). Defaults
/// to the executable directory so Windows installs stay portable; falls back
/// to the OS per-user data directory when the executable directory is not
/// writable (system-wide Linux installs, read-only macOS .app bundles).
pub(crate) fn base_dir_for(install_dir: &Path) -> PathBuf {
    if data_anchor_is_writable(install_dir) {
        return install_dir.to_path_buf();
    }
    if let Some(dir) = dirs::data_dir() {
        return dir.join(APP_IDENTIFIER);
    }
    install_dir.to_path_buf()
}

/// Best-effort base directory for code that runs before `AppPaths` exists
/// (emergency startup logging). Never fails while an OS data directory or
/// the executable directory can be resolved.
pub(crate) fn fallback_base_dir() -> Option<PathBuf> {
    match resolve_install_dir() {
        Ok(install_dir) => Some(base_dir_for(&install_dir)),
        Err(_) => dirs::data_dir().map(|dir| dir.join(APP_IDENTIFIER)),
    }
}

fn data_anchor_is_writable(dir: &Path) -> bool {
    use std::io::Write;

    let probe_dir = dir.join(DATA_DIR_NAME);
    if fs::create_dir_all(&probe_dir).is_err() {
        return false;
    }
    let probe_file = probe_dir.join(".write-probe");
    let probed = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe_file)
        .and_then(|mut file| file.write_all(b"probe"))
        .is_ok();
    let _ = fs::remove_file(&probe_file);
    probed
}
