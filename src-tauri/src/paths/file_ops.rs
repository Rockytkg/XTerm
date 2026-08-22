use std::path::{Path, PathBuf};

pub(super) fn normalize_configured_path(raw: &str, label: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is required"));
    }
    let resolved = if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        let home = dirs::home_dir()
            .ok_or_else(|| format!("failed to resolve home directory for {label}"))?;
        let rest = trimmed
            .trim_start_matches('~')
            .trim_start_matches(['/', '\\']);
        home.join(rest)
    } else {
        PathBuf::from(trimmed)
    };

    // Resolve `..`, `.`, and symlinks for a clean canonical path.
    // The caller is expected to `create_dir_all` before this returns
    // so the directory exists by the time canonicalize is called.
    resolve_dot_symlinks(&resolved)
}

/// Strips `..` and `.` components without requiring the path to exist on disk,
/// unlike `canonicalize`. This avoids the "file doesn't exist" error when
/// canonicalizing a path that hasn't been created yet.
fn resolve_dot_symlinks(path: &Path) -> Result<PathBuf, String> {
    let mut components = Vec::new();
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    if components.is_empty() {
        return Err("the resolved path is empty".to_string());
    }
    let mut out = PathBuf::new();
    for c in components {
        out.push(c);
    }
    Ok(out)
}
