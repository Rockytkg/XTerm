import { invokeDebugIpc, invokeLoggedIpc } from "./ipc/core";

export function closeSftpSession(connectionId, sessionId) {
  return invokeLoggedIpc("sftp_close_session", { request: { connectionId, sessionId } });
}

export function listRemoteSftp(connectionId, sessionId, path) {
  return invokeDebugIpc("sftp_list_remote", { request: { connectionId, sessionId, path } });
}

export function transferSftp(request) {
  return invokeLoggedIpc("sftp_transfer", { request });
}

export function listSftpTransfers(connectionId, sessionId) {
  return invokeLoggedIpc("sftp_transfer_list", { request: { connectionId, sessionId } });
}

export function pauseSftpTransfer(transferId) {
  return invokeLoggedIpc("sftp_transfer_pause", { request: { transferId } });
}

export function resumeSftpTransfer(transferId) {
  return invokeLoggedIpc("sftp_transfer_resume", { request: { transferId } });
}

export function cancelSftpTransfer(transferId) {
  return invokeLoggedIpc("sftp_transfer_cancel", { request: { transferId } });
}

export function chooseSftpDownloadPath(request) {
  return invokeLoggedIpc("sftp_choose_download_path", { request });
}

export function chooseSftpUploadFiles(request) {
  return invokeLoggedIpc("sftp_choose_upload_files", { request });
}

export function deleteRemoteSftp(connectionId, sessionId, paths) {
  return invokeLoggedIpc("sftp_delete", { request: { connectionId, sessionId, paths } });
}

export function createRemoteSftpDir(connectionId, sessionId, parentPath, name) {
  return invokeLoggedIpc("sftp_create_dir", {
    request: { connectionId, sessionId, parentPath, name },
  });
}

export function createRemoteSftpFile(connectionId, sessionId, parentPath, name) {
  return invokeLoggedIpc("sftp_create_file", {
    request: { connectionId, sessionId, parentPath, name },
  });
}

export function readRemoteSftpFile(connectionId, sessionId, path) {
  return invokeLoggedIpc("sftp_read_file", { request: { connectionId, sessionId, path } });
}

export function writeRemoteSftpFile(connectionId, sessionId, path, content) {
  return invokeLoggedIpc("sftp_write_file", {
    request: { connectionId, sessionId, path, content },
  });
}

export function statRemoteSftpFile(connectionId, sessionId, path) {
  return invokeDebugIpc("sftp_stat_file", { request: { connectionId, sessionId, path } });
}

export function renameRemoteSftp(
  connectionId,
  sessionId,
  fromPath,
  toParentPath,
  toName,
  conflictAction = "create",
) {
  return invokeLoggedIpc("sftp_rename", {
    request: { connectionId, sessionId, fromPath, toParentPath, toName, conflictAction },
  });
}
