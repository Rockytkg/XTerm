import { invokeDebugIpc, invokeDetailedIpc, invokeLoggedIpc } from "./ipc/core";
import { base64ToBytes, bytesFromIpcResult } from "../utils/filePreview";

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

// 预览内部子资源（如邮件附件）的保存：内容已在前端，由后端弹保存对话框并落盘
export function saveSftpPreviewResource(request) {
  return invokeLoggedIpc("sftp_save_preview_resource", { request });
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

export function readRemoteSftpFileBase64(connectionId, sessionId, path) {
  return invokeLoggedIpc("sftp_read_file_base64", { request: { connectionId, sessionId, path } });
}

// Tauri 原始字节响应仅在自定义协议 IPC（Windows/Linux）下为 ArrayBuffer；macOS/iOS 的
// WKWebView 只走 postMessage+JSON 通道，Raw 响应会退化为数字数组（比 base64 更差）。
// 首次读取前以空字节命令探测通道能力并缓存，不支持时回退 base64 读取。
let bytesIpcSupported;
async function supportsBytesIpc() {
  if (bytesIpcSupported === undefined) {
    try {
      bytesIpcSupported = (await invokeDebugIpc("sftp_ipc_bytes_probe")) instanceof ArrayBuffer;
    } catch {
      bytesIpcSupported = false;
    }
  }
  return bytesIpcSupported;
}

// 预览读取：优先原始字节通道，返回 Uint8Array
export async function readRemoteSftpFileBytes(connectionId, sessionId, path) {
  if (!(await supportsBytesIpc())) {
    return base64ToBytes(await readRemoteSftpFileBase64(connectionId, sessionId, path));
  }
  const result = await invokeDetailedIpc("sftp_read_file_bytes", {
    request: { connectionId, sessionId, path },
  });
  return bytesFromIpcResult(result);
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
