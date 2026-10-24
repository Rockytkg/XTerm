// 测试桩：替换 src/services/sftp.js。默认立即成功；handlers 可按测试注入 deferred 或错误。
const DEFAULT_STAT = { size: 5, modified: 1700000000, mime: "text/plain" };

export const mockSftpState = {
  stat: { ...DEFAULT_STAT },
  statCalls: 0,
  readCalls: 0,
  calls: [],
  handlers: {},
};

export function resetMockSftpState() {
  mockSftpState.stat = { ...DEFAULT_STAT };
  mockSftpState.statCalls = 0;
  mockSftpState.readCalls = 0;
  mockSftpState.calls.length = 0;
  mockSftpState.handlers = {};
}

function invokeMock(name, args, fallback) {
  mockSftpState.calls.push({ name, args });
  const handler = mockSftpState.handlers[name];
  return handler ? handler(...args) : fallback(...args);
}

export async function closeSftpSession(...args) {
  return invokeMock("closeSftpSession", args, () => undefined);
}

export async function listRemoteSftp(...args) {
  return invokeMock("listRemoteSftp", args, () => ({ entries: [], parent: null, path: "/" }));
}

export async function transferSftp(...args) {
  return invokeMock("transferSftp", args, (request) => request.transferId || null);
}

export async function listSftpTransfers(...args) {
  return invokeMock("listSftpTransfers", args, () => []);
}

export async function pauseSftpTransfer(...args) {
  return invokeMock("pauseSftpTransfer", args, () => undefined);
}

export async function resumeSftpTransfer(...args) {
  return invokeMock("resumeSftpTransfer", args, () => undefined);
}

export async function cancelSftpTransfer(...args) {
  return invokeMock("cancelSftpTransfer", args, () => undefined);
}

export async function chooseSftpDownloadPath(...args) {
  return invokeMock("chooseSftpDownloadPath", args, () => null);
}

export async function saveSftpPreviewResource(...args) {
  return invokeMock("saveSftpPreviewResource", args, () => undefined);
}

export async function chooseSftpUploadFiles(...args) {
  return invokeMock("chooseSftpUploadFiles", args, () => []);
}

export async function deleteRemoteSftp(...args) {
  return invokeMock("deleteRemoteSftp", args, () => undefined);
}

export async function createRemoteSftpDir(...args) {
  return invokeMock("createRemoteSftpDir", args, () => undefined);
}

export async function createRemoteSftpFile(...args) {
  return invokeMock("createRemoteSftpFile", args, () => undefined);
}

export async function readRemoteSftpFile(...args) {
  mockSftpState.readCalls += 1;
  return invokeMock("readRemoteSftpFile", args, () => "hello text");
}

export async function readRemoteSftpFileBase64(...args) {
  return invokeMock("readRemoteSftpFileBase64", args, () => "aGVsbG8=");
}

export async function readRemoteSftpFileBytes(...args) {
  return invokeMock("readRemoteSftpFileBytes", args, () =>
    Uint8Array.from("hello", (char) => char.charCodeAt(0)),
  );
}

export async function writeRemoteSftpFile(...args) {
  return invokeMock("writeRemoteSftpFile", args, () => ({ modified: 1700000001 }));
}

export async function statRemoteSftpFile(...args) {
  mockSftpState.statCalls += 1;
  return invokeMock("statRemoteSftpFile", args, () => mockSftpState.stat);
}

export async function renameRemoteSftp(...args) {
  return invokeMock("renameRemoteSftp", args, () => undefined);
}
