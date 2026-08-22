use sanitise_file_name::{sanitize_with_options, Options as SanitizeFileNameOptions};
use std::{
    future::Future,
    path::{Path, PathBuf},
};

use crate::terminal::internal::core::{ConnectionError, ConnectionResult, ResolvedConnection};

#[derive(Debug)]
pub(super) struct RemoteWorkingDirectory {
    pub(super) path: String,
}

pub(super) fn required<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

pub(crate) fn ensure_open_not_cancelled(request: &ResolvedConnection) -> ConnectionResult<()> {
    match request.open_scope.as_ref() {
        Some(scope) if scope.is_cancelled() => Err(ConnectionError::cancelled()),
        _ => Ok(()),
    }
}

pub(crate) fn ensure_open_current(
    state: &crate::state::AppState,
    request: &ResolvedConnection,
) -> ConnectionResult<()> {
    let open_request_id = request.open_request_id.as_deref().unwrap_or(&request.id);
    match request.open_scope.as_ref() {
        Some(scope) if state.connection_open_matches(open_request_id, scope) => Ok(()),
        Some(_) => Err(ConnectionError::new(
            "connection_open_superseded",
            "connection open was superseded",
            false,
        )),
        None => Ok(()),
    }
}

pub(crate) async fn cancelable_open<F, T>(
    request: &ResolvedConnection,
    operation: F,
) -> ConnectionResult<T>
where
    F: Future<Output = ConnectionResult<T>>,
{
    let Some(mut scope) = request.open_scope.clone() else {
        return operation.await;
    };
    if scope.is_cancelled() {
        return Err(ConnectionError::cancelled());
    }
    tokio::select! {
        _ = scope.cancelled() => Err(ConnectionError::cancelled()),
        result = operation => result,
    }
}

pub(super) fn normalize_terminal_transfer_name(value: &str, fallback: &str) -> String {
    let options = SanitizeFileNameOptions {
        normalise_whitespace: false,
        trim_more_punctuation: false,
        six_measures_of_barley: "",
        ..SanitizeFileNameOptions::DEFAULT
    };
    let name = sanitize_with_options(value.trim(), &options);
    if name.is_empty() {
        fallback.to_string()
    } else {
        name
    }
}

pub(super) fn unique_terminal_transfer_download_path(directory: &Path, file_name: &str) -> PathBuf {
    let path = directory.join(file_name);
    if !path.exists() {
        return path;
    }

    let file_path = Path::new(file_name);
    let stem = file_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(file_name);
    let extension = file_path.extension().and_then(|value| value.to_str());

    for index in 1..1000 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    path
}
