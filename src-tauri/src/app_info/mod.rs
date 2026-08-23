use semver::Version;
use serde::{Deserialize, Serialize};

const PROJECT_REPOSITORY_URL: &str = "https://github.com/Rockytkg/xterm";
const PROJECT_ISSUES_URL: &str = "https://github.com/Rockytkg/xterm/issues";
const PROJECT_AUTHOR_URL: &str = "https://github.com/Rockytkg";
const PROJECT_LICENSE_URL: &str = "https://github.com/Rockytkg/xterm/blob/main/LICENSE";
const PROJECT_LICENSE: &str = "MIT";
const PROJECT_AUTHOR: &str = "Rockytkg";
const APP_DISPLAY_NAME: &str = "XTerm";
const GITHUB_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/Rockytkg/xterm/releases/latest";

#[derive(Serialize)]
pub struct AppMetadata {
    name: &'static str,
    version: &'static str,
    author: &'static str,
    author_url: &'static str,
    license: &'static str,
    license_url: &'static str,
    repository_url: &'static str,
    issues_url: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAsset {
    name: String,
    download_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current_version: String,
    latest_version: Option<String>,
    update_available: bool,
    release_url: Option<String>,
    release_name: Option<String>,
    release_notes: Option<String>,
    published_at: Option<String>,
    assets: Vec<ReleaseAsset>,
    platform: &'static str,
    arch: &'static str,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

/// Returns compile-time app metadata used by the About screen.
///
/// The version comes from Cargo package metadata, so release builds display the
/// version that was compiled instead of a duplicated frontend constant.
#[tauri::command]
pub fn app_metadata() -> AppMetadata {
    AppMetadata {
        name: APP_DISPLAY_NAME,
        version: env!("CARGO_PKG_VERSION"),
        author: PROJECT_AUTHOR,
        author_url: PROJECT_AUTHOR_URL,
        license: PROJECT_LICENSE,
        license_url: PROJECT_LICENSE_URL,
        repository_url: PROJECT_REPOSITORY_URL,
        issues_url: PROJECT_ISSUES_URL,
    }
}

/// Checks the GitHub latest-release endpoint and compares it with the compiled
/// app version. This only detects availability; installation still belongs to a
/// future signed updater pipeline.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateStatus, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .user_agent(format!("xterm/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|error| format!("failed to create update client: {error}"))?;

    let release = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .send()
        .await
        .map_err(|error| format!("failed to request latest release: {error}"))?
        .error_for_status()
        .map_err(|error| format!("latest release request failed: {error}"))?
        .json::<GithubRelease>()
        .await
        .map_err(|error| format!("failed to parse latest release: {error}"))?;

    if release.draft || release.prerelease {
        log::info!(
            target: "app.update",
            "latest GitHub release is draft or prerelease; ignoring update signal"
        );
        return Ok(UpdateStatus {
            current_version,
            latest_version: None,
            update_available: false,
            release_url: None,
            release_name: None,
            release_notes: None,
            published_at: None,
            assets: Vec::new(),
            platform: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        });
    }

    let update_available = is_remote_version_newer(&current_version, &release.tag_name);
    log::info!(
        target: "app.update",
        "checked updates: current={}, latest={}, available={}",
        current_version,
        release.tag_name,
        update_available
    );

    Ok(UpdateStatus {
        current_version,
        latest_version: Some(normalize_version(&release.tag_name)),
        update_available,
        release_url: release.html_url,
        release_name: release.name,
        release_notes: release.body,
        published_at: release.published_at,
        assets: release
            .assets
            .into_iter()
            .filter_map(|asset| {
                asset.browser_download_url.map(|url| ReleaseAsset {
                    name: asset.name,
                    download_url: url,
                })
            })
            .collect(),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    })
}

/// Restarts the app after a setting change that only takes effect at startup.
#[tauri::command]
pub fn app_restart(app: tauri::AppHandle) {
    log::info!(target: "app.update", "restarting application by user request");
    app.restart();
}

fn normalize_version(raw: &str) -> String {
    raw.trim().trim_start_matches('v').to_string()
}

fn is_remote_version_newer(current: &str, remote: &str) -> bool {
    match (
        Version::parse(&normalize_version(current)),
        Version::parse(&normalize_version(remote)),
    ) {
        (Ok(current), Ok(remote)) => remote > current,
        _ => normalize_version(remote) != normalize_version(current),
    }
}
