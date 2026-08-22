use super::{sftp::normalize_remote_path, util::RemoteWorkingDirectory};

pub(super) fn extract_working_directories(data: &str) -> Vec<RemoteWorkingDirectory> {
    let mut directories = Vec::new();
    let mut remaining = data;
    while let Some(index) = remaining.find("\x1b]") {
        remaining = &remaining[index + 2..];
        let Some((payload, rest)) = split_osc_payload(remaining) else {
            break;
        };
        if let Some(directory) = parse_working_directory_payload(payload) {
            directories.push(directory);
        }
        remaining = rest;
    }
    directories
}

fn split_osc_payload(value: &str) -> Option<(&str, &str)> {
    let bell = value.find('\x07');
    let st = value.find("\x1b\\");
    match (bell, st) {
        (Some(bell), Some(st)) if bell < st => Some((&value[..bell], &value[bell + 1..])),
        (Some(_), Some(st)) => Some((&value[..st], &value[st + 2..])),
        (Some(bell), None) => Some((&value[..bell], &value[bell + 1..])),
        (None, Some(st)) => Some((&value[..st], &value[st + 2..])),
        (None, None) => None,
    }
}

fn parse_working_directory_payload(payload: &str) -> Option<RemoteWorkingDirectory> {
    let path = parse_tabby_current_dir(payload)?;
    Some(RemoteWorkingDirectory {
        path: normalize_remote_path(&path),
    })
}

fn parse_tabby_current_dir(payload: &str) -> Option<String> {
    payload
        .strip_prefix("1337;CurrentDir=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
