use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct AppPaths {
    pub(super) install_dir: PathBuf,
    pub(super) base_dir: PathBuf,
    pub(super) data_dir: PathBuf,
    pub(super) log_dir: PathBuf,
}

#[cfg(test)]
impl AppPaths {
    /// Synthetic paths rooted at a throwaway directory, for tests that need
    /// an `AppState` without touching real user directories.
    pub(crate) fn for_tests(root: PathBuf) -> Self {
        Self {
            install_dir: root.clone(),
            base_dir: root.clone(),
            data_dir: root.join("data"),
            log_dir: root.join("logs"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathSettings {
    pub install_dir: String,
    pub data_dir: String,
    pub log_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryPickerRequest {
    pub(super) default_path: Option<String>,
    pub(super) title: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistedPathSettings {
    pub(super) data_dir: Option<PathBuf>,
    pub(super) log_dir: Option<PathBuf>,
}
