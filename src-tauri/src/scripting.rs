use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::logging;

const MAX_SCRIPT_FETCH_BYTES: u64 = 1024 * 1024;
// 脚本数据文件的读写上限：防止脚本把超大文件读进前端内存或写爆磁盘。
const MAX_SCRIPT_DATA_FILE_BYTES: u64 = 32 * 1024 * 1024;
const SCRIPT_DATA_TEXT_EXTENSIONS: [&str; 7] = ["txt", "log", "csv", "json", "xml", "yaml", "yml"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PickedScriptFile {
    name: String,
    code: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScriptDialogLabels {
    title: String,
    js_files_label: String,
    all_files_label: String,
}

/// 打开原生文件选择器选一个 .js 脚本并读回内容；
/// 用户取消时返回 None。对话框文案由前端按当前语言传入。
#[tauri::command]
pub(crate) async fn script_pick_file(
    app: AppHandle,
    labels: ScriptDialogLabels,
) -> Result<Option<PickedScriptFile>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(labels.title)
        .add_filter(labels.js_files_label, &["js"])
        .add_filter(labels.all_files_label, &["*"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver
        .await
        .map_err(|_| "script file picker closed before returning a result".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve selected file: {error}"))?;
    let code = tokio::fs::read_to_string(&path).await.map_err(|error| {
        format!(
            "failed to read script file '{}': {error}",
            path.to_string_lossy()
        )
    })?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    logging::event("scripting.commands", "script_pick_file")
        .field("path", path.to_string_lossy())
        .info();
    Ok(Some(PickedScriptFile { name, code }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScriptExportRequest {
    file_name: String,
    code: String,
    labels: ScriptDialogLabels,
}

/// 弹出原生保存对话框把脚本导出为 .js 文件；用户取消返回 None，
/// 成功时返回写入路径。对话框文案由前端按当前语言传入。
#[tauri::command]
pub(crate) async fn script_export_file(
    app: AppHandle,
    request: ScriptExportRequest,
) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(request.labels.title)
        .set_file_name(&request.file_name)
        .add_filter(request.labels.js_files_label, &["js"])
        .add_filter(request.labels.all_files_label, &["*"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver
        .await
        .map_err(|_| "script export picker closed before returning a result".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve export path: {error}"))?;
    tokio::fs::write(&path, request.code.as_bytes())
        .await
        .map_err(|error| {
            format!(
                "failed to write script file '{}': {error}",
                path.to_string_lossy()
            )
        })?;
    let exported = path.to_string_lossy().to_string();
    logging::event("scripting.commands", "script_export_file")
        .field("path", &exported)
        .info();
    Ok(Some(exported))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataFileDialogLabels {
    title: String,
    text_files_label: String,
    all_files_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadDataFileResult {
    name: String,
    content: String,
}

/// 脚本数据读取：弹出原生文件选择器，由用户亲自选定要读取的本地文件。
/// 脚本无法指定路径，只能拿到用户授权的那一个文件的文本内容；
/// 用户取消时返回 None。限制文件大小且只接受 UTF-8 文本，纯数据读取，不涉及任何执行。
#[tauri::command]
pub(crate) async fn script_read_data_file(
    app: AppHandle,
    labels: DataFileDialogLabels,
) -> Result<Option<ReadDataFileResult>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(labels.title)
        .add_filter(labels.text_files_label, &SCRIPT_DATA_TEXT_EXTENSIONS)
        .add_filter(labels.all_files_label, &["*"])
        .pick_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver
        .await
        .map_err(|_| "data file picker closed before returning a result".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve selected file: {error}"))?;
    let metadata = tokio::fs::metadata(&path).await.map_err(|error| {
        format!(
            "failed to stat data file '{}': {error}",
            path.to_string_lossy()
        )
    })?;
    if !metadata.is_file() {
        return Err("selected path is not a regular file".to_string());
    }
    if metadata.len() > MAX_SCRIPT_DATA_FILE_BYTES {
        return Err(format!(
            "data file is too large (limit {} bytes)",
            MAX_SCRIPT_DATA_FILE_BYTES
        ));
    }
    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        format!(
            "failed to read data file '{}': {error}",
            path.to_string_lossy()
        )
    })?;
    let content =
        String::from_utf8(bytes).map_err(|_| "data file is not valid utf-8 text".to_string())?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    logging::event("scripting.commands", "script_read_data_file")
        .field("path", path.to_string_lossy())
        .info();
    Ok(Some(ReadDataFileResult { name, content }))
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteDataFileRequest {
    file_name: String,
    content: String,
    labels: DataFileDialogLabels,
}

/// 脚本数据保存：弹出原生保存对话框，写入路径完全由用户在系统对话框中决定，
/// 脚本只能提供建议文件名与文本内容；用户取消时返回 None，成功时返回写入路径。
/// 仅做纯文本写入，不执行任何命令。
#[tauri::command]
pub(crate) async fn script_write_data_file(
    app: AppHandle,
    request: WriteDataFileRequest,
) -> Result<Option<String>, String> {
    if request.content.len() as u64 > MAX_SCRIPT_DATA_FILE_BYTES {
        return Err(format!(
            "data content is too large (limit {} bytes)",
            MAX_SCRIPT_DATA_FILE_BYTES
        ));
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(request.labels.title)
        .set_file_name(&request.file_name)
        .add_filter(
            request.labels.text_files_label,
            &SCRIPT_DATA_TEXT_EXTENSIONS,
        )
        .add_filter(request.labels.all_files_label, &["*"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    let Some(path) = receiver
        .await
        .map_err(|_| "data file save picker closed before returning a result".to_string())?
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| format!("failed to resolve save path: {error}"))?;
    tokio::fs::write(&path, request.content.as_bytes())
        .await
        .map_err(|error| {
            format!(
                "failed to write data file '{}': {error}",
                path.to_string_lossy()
            )
        })?;
    let saved = path.to_string_lossy().to_string();
    logging::event("scripting.commands", "script_write_data_file")
        .field("path", &saved)
        .info();
    Ok(Some(saved))
}

/// 拉取远程脚本内容（油猴式 @updateURL 更新检测）。仅允许 http/https，
/// 限制响应大小与超时，避免脚本库被当作通用抓取器滥用。
#[tauri::command]
pub(crate) async fn script_fetch_text(url: String) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(url.trim()).map_err(|error| format!("invalid script url: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("script url must be http or https".to_string());
    }
    let client = reqwest::Client::builder()
        .user_agent(format!("xterm/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| format!("failed to create script fetch client: {error}"))?;
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|error| format!("failed to fetch script: {error}"))?
        .error_for_status()
        .map_err(|error| format!("script fetch failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SCRIPT_FETCH_BYTES)
    {
        return Err("script response is too large".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("failed to read script response: {error}"))?;
    if bytes.len() as u64 > MAX_SCRIPT_FETCH_BYTES {
        return Err("script response is too large".to_string());
    }
    String::from_utf8(bytes.to_vec()).map_err(|error| format!("script is not valid utf-8: {error}"))
}
