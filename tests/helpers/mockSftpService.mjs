// 测试桩：替换 src/services/sftp.js，模拟后端 IPC 立即成功返回。
// mockSftpState 允许单个测试覆盖响应，用后必须用 resetMockSftpState 恢复默认。
const DEFAULT_STAT = { size: 5, modified: 1700000000, mime: "text/plain" };

export const mockSftpState = {
  stat: { ...DEFAULT_STAT },
};

export function resetMockSftpState() {
  mockSftpState.stat = { ...DEFAULT_STAT };
}

export function closeSftpSession() {
  return Promise.resolve();
}
export function listRemoteSftp() {
  return Promise.resolve({ entries: [], parentPath: null, path: "/" });
}
export function transferSftp() {
  return Promise.resolve();
}
export function listSftpTransfers() {
  return Promise.resolve([]);
}
export function pauseSftpTransfer() {
  return Promise.resolve();
}
export function resumeSftpTransfer() {
  return Promise.resolve();
}
export function cancelSftpTransfer() {
  return Promise.resolve();
}
export function chooseSftpDownloadPath() {
  return Promise.resolve(null);
}
export function chooseSftpUploadFiles() {
  return Promise.resolve([]);
}
export function deleteRemoteSftp() {
  return Promise.resolve();
}
export function createRemoteSftpDir() {
  return Promise.resolve();
}
export function createRemoteSftpFile() {
  return Promise.resolve();
}
export function readRemoteSftpFile() {
  return Promise.resolve("hello text");
}
export function readRemoteSftpFileBase64() {
  return Promise.resolve("aGVsbG8=");
}
export function readRemoteSftpFileBytes() {
  return Promise.resolve(Uint8Array.from("hello", (char) => char.charCodeAt(0)));
}
export function writeRemoteSftpFile() {
  return Promise.resolve({ modified: 1700000001 });
}
export function statRemoteSftpFile() {
  return Promise.resolve(mockSftpState.stat);
}
export function renameRemoteSftp() {
  return Promise.resolve();
}
