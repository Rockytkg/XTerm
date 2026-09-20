use semver::Version;
use serde::{Deserialize, Serialize};

const PROJECT_REPOSITORY_URL: &str = "https://github.com/Rockytkg/xterm";
const PROJECT_ISSUES_URL: &str = "https://github.com/Rockytkg/xterm/issues";
const PROJECT_AUTHOR_URL: &str = "https://github.com/Rockytkg";
const PROJECT_LICENSE_URL: &str = "https://github.com/Rockytkg/xterm/blob/main/LICENSE";
const PROJECT_LICENSE: &str = "MIT";
const PROJECT_AUTHOR: &str = "Rockytkg";
const APP_DISPLAY_NAME: &str = "XTerm";
const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/Rockytkg/xterm/releases?per_page=30";

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

#[derive(Deserialize, Clone)]
struct GithubAsset {
    name: String,
    browser_download_url: Option<String>,
}

#[derive(Deserialize, Clone)]
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

/// Checks the GitHub releases feed and compares it with the compiled app
/// version. This only detects availability; installation still belongs to a
/// future signed updater pipeline.
///
/// 通道规则(dev 与 rel 相结合):
/// - 正式版只与正式版比较(标准 semver),不接收 dev prerelease 推送;
/// - dev 快照(版本含 `-dev.N`)只认两类更新:基版本更高的正式版(rel 覆盖),
///   或基版本更高/同基版本序号更大的 dev 快照。同基版本的正式版不算更新——
///   按 semver 它高于 `-dev.N` prerelease,但快照构建自该 tag 之后的代码。
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateStatus, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let client = reqwest::Client::builder()
        .user_agent(format!("xterm/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|error| format!("failed to create update client: {error}"))?;

    let releases = client
        .get(GITHUB_RELEASES_URL)
        .send()
        .await
        .map_err(|error| format!("failed to request releases: {error}"))?
        .error_for_status()
        .map_err(|error| format!("releases request failed: {error}"))?
        .json::<Vec<GithubRelease>>()
        .await
        .map_err(|error| format!("failed to parse releases: {error}"))?;

    let status = select_update(&current_version, releases);
    log::info!(
        target: "app.update",
        "checked updates: current={}, latest={:?}, available={}",
        status.current_version,
        status.latest_version,
        status.update_available
    );
    Ok(status)
}

fn select_update(current_raw: &str, releases: Vec<GithubRelease>) -> UpdateStatus {
    let current = Version::parse(&normalize_version(current_raw)).ok();
    let current_is_dev = current.as_ref().and_then(dev_build_number).is_some();

    // latest_seen 用于"已是最新"时仍展示最近一次发布;best_newer 是真正命中更新的候选。
    // 候选间排序见 remote_rank:基版本 > 正式版优先于 dev > dev 序号。
    let mut latest_seen: Option<(GithubRelease, Version)> = None;
    let mut best_newer: Option<(GithubRelease, Version)> = None;
    for release in releases {
        // draft 一律忽略;正式版用户不接收 dev prerelease 推送
        if release.draft || (release.prerelease && !current_is_dev) {
            continue;
        }
        let Ok(remote) = Version::parse(&normalize_version(&release.tag_name)) else {
            continue;
        };
        let newer = match &current {
            Some(current) => is_remote_version_newer(current, &remote),
            // 当前版本号无法解析时退化为字符串不等比较,避免永远提示或永不提示
            None => normalize_version(&release.tag_name) != normalize_version(current_raw),
        };
        if latest_seen
            .as_ref()
            .is_none_or(|(_, seen)| remote_rank(&remote) > remote_rank(seen))
        {
            latest_seen = Some((release.clone(), remote.clone()));
        }
        if newer
            && best_newer
                .as_ref()
                .is_none_or(|(_, best)| remote_rank(&remote) > remote_rank(best))
        {
            best_newer = Some((release, remote));
        }
    }

    let (chosen, update_available) = match best_newer {
        Some(best) => (Some(best), true),
        None => (latest_seen, false),
    };
    let (latest_version, release_url, release_name, release_notes, published_at, assets) =
        match chosen {
            Some((release, remote)) => (
                Some(remote.to_string()),
                release.html_url,
                release.name,
                release.body,
                release.published_at,
                release
                    .assets
                    .into_iter()
                    .filter_map(|asset| {
                        asset.browser_download_url.map(|url| ReleaseAsset {
                            name: asset.name,
                            download_url: url,
                        })
                    })
                    .collect(),
            ),
            None => (None, None, None, None, None, Vec::new()),
        };

    UpdateStatus {
        current_version: current_raw.to_string(),
        latest_version,
        update_available,
        release_url,
        release_name,
        release_notes,
        published_at,
        assets,
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
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

/// 提取 dev 快照序号:`1.0.5-dev.123+123` -> `Some(123)`,其它版本 -> `None`。
fn dev_build_number(version: &Version) -> Option<u64> {
    version.pre.as_str().strip_prefix("dev.")?.parse().ok()
}

fn base_version(version: &Version) -> Version {
    Version::new(version.major, version.minor, version.patch)
}

/// 候选版本排序键:基版本越高越新;同基版本正式版优先于 dev 快照;dev 之间比序号。
fn remote_rank(version: &Version) -> (u64, u64, u64, u8, u64) {
    (
        version.major,
        version.minor,
        version.patch,
        u8::from(dev_build_number(version).is_none()),
        dev_build_number(version).unwrap_or(0),
    )
}

fn is_remote_version_newer(current: &Version, remote: &Version) -> bool {
    match (dev_build_number(current), dev_build_number(remote)) {
        // dev 当前版本:正式版只有基版本更高才算"rel 覆盖"(同基版本正式版发布于
        // 快照所基于的 tag,代码并不更新)
        (Some(_), None) => *remote > base_version(current),
        // dev 对 dev:基版本更高,或同基版本序号更大
        (Some(current_n), Some(remote_n)) => {
            let (current_base, remote_base) = (base_version(current), base_version(remote));
            remote_base > current_base || (remote_base == current_base && remote_n > current_n)
        }
        (None, None) => remote > current,
        // 正式版用户不接收 dev 快照(调用方已按 prerelease 标记过滤,这里兜底)
        (None, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> Version {
        Version::parse(raw).unwrap()
    }

    #[test]
    fn dev_current_ignores_same_base_stable() {
        // 修复的核心场景:基于 v1.0.5 之后代码的快照不应再提示 v1.0.5 是"新版本"
        assert!(!is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.5")
        ));
        assert!(!is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.4")
        ));
    }

    #[test]
    fn dev_current_accepts_higher_stable() {
        assert!(is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.6")
        ));
        assert!(is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("2.0.0")
        ));
    }

    #[test]
    fn dev_current_compares_dev_by_base_then_number() {
        assert!(is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.5-dev.124+124")
        ));
        assert!(!is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.5-dev.123+123")
        ));
        assert!(!is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.5-dev.122+122")
        ));
        assert!(is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.6-dev.130+130")
        ));
        assert!(!is_remote_version_newer(
            &v("1.0.5-dev.123+123"),
            &v("1.0.4-dev.200+200")
        ));
    }

    #[test]
    fn stable_current_uses_plain_semver() {
        assert!(is_remote_version_newer(&v("1.0.5"), &v("1.0.6")));
        assert!(!is_remote_version_newer(&v("1.0.5"), &v("1.0.5")));
        assert!(!is_remote_version_newer(&v("1.0.5"), &v("1.0.4")));
        // 正式版用户永不接收 dev 快照
        assert!(!is_remote_version_newer(
            &v("1.0.5"),
            &v("1.0.6-dev.130+130")
        ));
    }

    #[test]
    fn remote_rank_prefers_higher_base_then_stable_then_number() {
        assert!(remote_rank(&v("1.0.6")) > remote_rank(&v("1.0.6-dev.130+130")));
        assert!(remote_rank(&v("1.0.6-dev.130+130")) > remote_rank(&v("1.0.5")));
        assert!(remote_rank(&v("1.0.5-dev.124+124")) > remote_rank(&v("1.0.5-dev.123+123")));
    }

    #[test]
    fn dev_build_number_parsing() {
        assert_eq!(dev_build_number(&v("1.0.5-dev.123+123")), Some(123));
        assert_eq!(dev_build_number(&v("1.0.5")), None);
        assert_eq!(dev_build_number(&v("1.0.5-beta.1")), None);
    }
}
