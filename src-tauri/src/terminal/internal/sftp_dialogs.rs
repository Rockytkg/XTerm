use crate::session_recording::{dialog_file_name, dialog_label};
use crate::terminal::internal::core::{
    SftpChooseDownloadPathRequest, SftpChooseUploadFilesRequest,
    TrzszChooseDownloadDirectoryRequest, TrzszChooseUploadFilesRequest,
};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_dialog::FilePath;

async fn wait_for_dialog_result<T>(
    receiver: tokio::sync::oneshot::Receiver<Option<T>>,
    action: &str,
) -> Result<Option<T>, String> {
    receiver
        .await
        .map_err(|_| format!("{action} dialog closed before returning a result"))
}

fn file_path_to_string(path: FilePath, error_context: &str) -> Result<String, String> {
    path.into_path()
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| format!("{error_context}: {error}"))
}

pub(super) async fn choose_sftp_download_path(
    app: &AppHandle,
    request: &SftpChooseDownloadPathRequest,
) -> Result<Option<String>, String> {
    let title = dialog_label(&request.title, "Choose download destination");
    let safe_name = dialog_file_name(&request.default_file_name, "download");

    // Keep SFTP path selection on Tauri's dialog plugin instead of launching a
    // separate WinForms process. This keeps the panel parented to the app and
    // lets the OS provide the same localized, app-branded file UI as other
    // WebView/Tauri file pickers.
    if request.kind == "dir" {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_title(title)
            .pick_folder(move |path| {
                let _ = sender.send(path);
            });
        return wait_for_dialog_result(receiver, "SFTP download folder")
            .await?
            .map(|path| {
                path.into_path()
                    .map(|path| path.join(&safe_name).to_string_lossy().to_string())
                    .map_err(|error| format!("failed to resolve folder path: {error}"))
            })
            .transpose();
    }

    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(safe_name)
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    wait_for_dialog_result(receiver, "SFTP download file")
        .await?
        .map(|path| file_path_to_string(path, "failed to resolve save path"))
        .transpose()
}

pub(super) async fn choose_sftp_upload_files(
    app: &AppHandle,
    request: &SftpChooseUploadFilesRequest,
) -> Result<Vec<String>, String> {
    let title = dialog_label(&request.title, "Choose files to upload");
    let all_files_label = dialog_label(&request.all_files_label, "All files");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(all_files_label, &["*"])
        .pick_files(move |paths| {
            let _ = sender.send(paths);
        });
    wait_for_dialog_result(receiver, "SFTP upload files")
        .await?
        .unwrap_or_default()
        .into_iter()
        .map(|path| file_path_to_string(path, "failed to resolve upload path"))
        .collect()
}

pub(super) async fn choose_trzsz_upload_files(
    app: &AppHandle,
    request: &TrzszChooseUploadFilesRequest,
) -> Result<Vec<String>, String> {
    let title = dialog_label(&request.title, "Choose files to transfer");
    let all_files_label = dialog_label(&request.all_files_label, "All files");
    if request.directory {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_title(title)
            .pick_folder(move |path| {
                let _ = sender.send(path);
            });
        let Some(path) = wait_for_dialog_result(receiver, "upload folder").await? else {
            return Ok(Vec::new());
        };
        return file_path_to_string(path, "failed to resolve upload path").map(|path| vec![path]);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .add_filter(all_files_label, &["*"])
        .pick_files(move |paths| {
            let _ = sender.send(paths);
        });
    wait_for_dialog_result(receiver, "upload files")
        .await?
        .unwrap_or_default()
        .into_iter()
        .map(|path| file_path_to_string(path, "failed to resolve upload path"))
        .collect()
}

pub(super) async fn choose_trzsz_save_directory(
    app: &AppHandle,
    request: &TrzszChooseDownloadDirectoryRequest,
) -> Result<Option<String>, String> {
    let title = dialog_label(&request.title, "Choose transfer download folder");
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |path| {
            let _ = sender.send(path);
        });
    wait_for_dialog_result(receiver, "download directory")
        .await?
        .map(|path| file_path_to_string(path, "failed to resolve save directory"))
        .transpose()
}
