import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/sftpTransfersTestLoader.mjs", import.meta.url);

const { createRenderer, nextTick, reactive, ref } = await import("vue");
const { mockSftpState, resetMockSftpState } = await import("./helpers/mockSftpService.mjs");
const {
  emitSftpTransferProgress,
  flushSftpTransferFrames,
  resetSftpTransfersTestState,
  sftpTransfersTestState,
} = await import("./helpers/sftpTransfersTestMocks.mjs");
const { useSftpTransfers } = await import("../src/composables/useSftpTransfers.js");

const renderer = createRenderer({
  patchProp() {},
  insert(child, parent) {
    (parent.children || (parent.children = [])).push(child);
  },
  remove() {},
  createElement: () => ({}),
  createText: (text) => ({ text }),
  createComment: (text) => ({ text }),
  setText() {},
  setElementText() {},
  parentNode: () => null,
  nextSibling: () => null,
  querySelector: () => null,
  setScopeId() {},
  cloneNode: (node) => ({ ...node }),
  insertStaticContent: () => [{}, {}],
});

const mountedApps = new Set();
const t = (key, params) => (params?.name ? `${key}:${params.name}` : key);

function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function entry(name, kind = "file") {
  return { name, path: `/remote/${name}`, kind };
}

function backendItem(transferId, overrides = {}) {
  return {
    transferId,
    name: `${transferId}.txt`,
    direction: "download",
    transferred: 0,
    total: 10,
    status: "running",
    error: null,
    ...overrides,
  };
}

function calls(name) {
  return mockSftpState.calls.filter((call) => call.name === name);
}

async function mountTransfers(overrides = {}) {
  const props =
    overrides.props ||
    reactive({
      connection: { id: "c1", capabilities: { sftp: true } },
      sessionId: "s1",
      visible: false,
    });
  const selectedEntry = overrides.selectedEntry || ref(entry("a.txt"));
  const selectedNames = overrides.selectedNames || ref(new Set(["a.txt", "b.txt"]));
  const remotePath = overrides.remotePath || ref("/remote");
  let api;
  const app = renderer.createApp({
    setup() {
      api = useSftpTransfers({
        props,
        remotePath,
        selectedEntry,
        selectedNames,
        t,
        ...overrides,
      });
      return () => null;
    },
  });
  app.mount({});
  mountedApps.add(app);
  await settle();
  return { api, app, props, remotePath, selectedEntry, selectedNames };
}

async function startItem(api, name) {
  await api.startDownload(entry(name));
  const item = api.transfers.value.find((transfer) => transfer.name === name);
  assert.ok(item);
  return item;
}

test.beforeEach(() => {
  resetMockSftpState();
  resetSftpTransfersTestState();
  mockSftpState.handlers.chooseSftpDownloadPath = ({ defaultFileName }) =>
    `/downloads/${defaultFileName}`;
});

test.afterEach(() => {
  for (const app of mountedApps) app.unmount();
  mountedApps.clear();
});

test("预生成 ID 先入列，100 个快速 done 不受恢复事件缓冲上限影响", async () => {
  const { api } = await mountTransfers();
  const ids = [];
  const registeredBeforeStart = [];
  mockSftpState.handlers.transferSftp = (request) => {
    ids.push(request.transferId);
    registeredBeforeStart.push(api.transfers.value.some((item) => item.id === request.transferId));
    emitSftpTransferProgress({
      transferId: request.transferId,
      transferred: 10,
      total: 10,
      done: true,
      status: "done",
      error: null,
    });
    return request.transferId;
  };

  await Promise.all(
    Array.from({ length: 100 }, (_, index) => api.startDownload(entry(`file-${index}.txt`))),
  );
  flushSftpTransferFrames();

  assert.equal(ids.length, 100);
  assert.equal(new Set(ids).size, 100);
  assert.ok(ids.every(Boolean));
  assert.ok(registeredBeforeStart.every(Boolean));
  assert.equal(api.transfers.value.length, 50);
  assert.ok(api.transfers.value.every((item) => item.status === "done"));
  assert.ok(api.transfers.value.every((item) => item.progress === 100));
  assert.equal(api.activeTransfers.value, 0);
});

test("onMounted 先监听，done 先于 deferred list 返回时快照不回滚终态", async () => {
  const list = deferred();
  const observeCountsAtList = [];
  mockSftpState.handlers.listSftpTransfers = () => {
    observeCountsAtList.push(sftpTransfersTestState.observeCalls);
    return list.promise;
  };
  const { api } = await mountTransfers();

  assert.deepEqual(observeCountsAtList, [1]);
  emitSftpTransferProgress({
    transferId: "recovered",
    transferred: 10,
    total: 10,
    done: true,
    status: "done",
    error: null,
  });
  flushSftpTransferFrames();
  assert.equal(api.transfers.value.length, 0);

  list.resolve([backendItem("recovered", { transferred: 2, status: "running" })]);
  await settle();

  const item = api.transfers.value.find((transfer) => transfer.id === "recovered");
  assert.equal(item.status, "done");
  assert.equal(item.transferred, 10);
  assert.equal(item.progress, 100);
});

test("deferred list 先返回再收到 done 时仍保留恢复任务终态", async () => {
  const list = deferred();
  mockSftpState.handlers.listSftpTransfers = () => list.promise;
  const { api } = await mountTransfers();
  list.resolve([backendItem("recovered", { transferred: 2 })]);
  await settle();
  assert.equal(api.transfers.value[0].status, "running");

  emitSftpTransferProgress({
    transferId: "recovered",
    transferred: 10,
    total: 10,
    done: true,
    status: "done",
    error: null,
  });
  flushSftpTransferFrames();

  assert.equal(api.transfers.value[0].status, "done");
  assert.equal(api.transfers.value[0].transferred, 10);
  assert.equal(api.transfers.value[0].progress, 100);
});

test("deferred list 不回滚已收到的非终态进度", async () => {
  mockSftpState.handlers.listSftpTransfers = () => [backendItem("recovered", { transferred: 2 })];
  const { api, props } = await mountTransfers();
  const list = deferred();
  mockSftpState.handlers.listSftpTransfers = () => list.promise;
  props.visible = true;
  await nextTick();

  emitSftpTransferProgress({
    transferId: "recovered",
    transferred: 7,
    total: 10,
    done: false,
    status: "running",
    error: null,
  });
  flushSftpTransferFrames();
  list.resolve([backendItem("recovered", { transferred: 2 })]);
  await settle();

  assert.equal(api.transfers.value[0].status, "running");
  assert.equal(api.transfers.value[0].transferred, 7);
  assert.equal(api.transfers.value[0].progress, 70);
});

test("downloadEntry 显式下载传入项而不是多选中的其他选中项", async () => {
  const { api, selectedNames } = await mountTransfers();
  let closed = 0;

  await api.downloadEntry(entry("b.txt"), () => {
    closed += 1;
  });

  assert.equal(closed, 1);
  assert.deepEqual([...selectedNames.value].sort(), ["a.txt", "b.txt"]);
  assert.equal(calls("chooseSftpDownloadPath")[0].args[0].defaultFileName, "b.txt");
  assert.equal(calls("transferSftp")[0].args[0].remotePath, "/remote/b.txt");
  assert.equal(api.transfers.value[0].name, "b.txt");
});

test("pause/resume/cancel reject 恢复原状态，后续健康进度清除错误", async () => {
  const { api } = await mountTransfers();
  const item = await startItem(api, "controls.txt");
  mockSftpState.handlers.pauseSftpTransfer = () => Promise.reject(new Error("pause rejected"));
  mockSftpState.handlers.resumeSftpTransfer = () => Promise.reject(new Error("resume rejected"));
  mockSftpState.handlers.cancelSftpTransfer = () => Promise.reject(new Error("cancel rejected"));

  await api.pauseTransfer(item.id);
  assert.equal(api.transfers.value[0].status, "running");
  assert.match(api.transfers.value[0].error, /pause rejected/);
  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 4,
    total: 10,
    done: false,
    status: "running",
    error: null,
  });
  flushSftpTransferFrames();
  assert.equal(api.transfers.value[0].error, "");

  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 4,
    total: 10,
    done: false,
    status: "paused",
    error: null,
  });
  flushSftpTransferFrames();
  await api.resumeTransfer(item.id);
  assert.equal(api.transfers.value[0].status, "paused");
  assert.match(api.transfers.value[0].error, /resume rejected/);
  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 5,
    total: 10,
    done: false,
    status: "running",
    error: null,
  });
  flushSftpTransferFrames();
  assert.equal(api.transfers.value[0].error, "");

  await api.cancelTransfer(item.id);
  assert.equal(api.transfers.value[0].status, "running");
  assert.match(api.transfers.value[0].error, /cancel rejected/);
  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 6,
    total: 10,
    done: false,
    status: "running",
    error: null,
  });
  flushSftpTransferFrames();
  assert.equal(api.transfers.value[0].error, "");
  assert.equal(sftpTransfersTestState.toasts.length, 3);
});

test("failed/canceled 更新最终字节，终态 remove/clear 仅本地删除", async () => {
  const { api } = await mountTransfers();
  const done = await startItem(api, "done.txt");
  const failed = await startItem(api, "failed.txt");
  const canceled = await startItem(api, "canceled.txt");
  const active = await startItem(api, "active.txt");
  emitSftpTransferProgress({
    transferId: done.id,
    transferred: 10,
    total: 10,
    done: true,
    status: "done",
    error: null,
  });
  emitSftpTransferProgress({
    transferId: failed.id,
    transferred: 3,
    total: 10,
    done: true,
    status: "failed",
    error: "boom",
  });
  emitSftpTransferProgress({
    transferId: canceled.id,
    transferred: 6,
    total: 10,
    done: true,
    status: "canceled",
    error: "canceled",
  });
  flushSftpTransferFrames();

  const byId = (id) => api.transfers.value.find((item) => item.id === id);
  assert.equal(byId(done.id).progress, 100);
  assert.equal(byId(failed.id).transferred, 3);
  assert.equal(byId(failed.id).total, 10);
  assert.equal(byId(failed.id).progress, 30);
  assert.equal(byId(failed.id).error, "boom");
  assert.equal(byId(canceled.id).transferred, 6);
  assert.equal(byId(canceled.id).total, 10);
  assert.equal(byId(canceled.id).progress, 60);
  assert.equal(byId(canceled.id).error, "");

  await api.cancelTransfer(failed.id);
  await api.removeTransfer(done.id);
  api.clearCompletedTransfers();

  assert.equal(calls("cancelSftpTransfer").length, 0);
  assert.deepEqual(
    api.transfers.value.map((item) => item.id),
    [active.id],
  );
});

test("cancel 成功后等待 canceled 事件，不提前丢弃最终字节", async () => {
  const { api } = await mountTransfers();
  const item = await startItem(api, "cancel.txt");

  await api.cancelTransfer(item.id);
  assert.equal(api.transfers.value[0].status, "running");
  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 4,
    total: 10,
    done: true,
    status: "canceled",
    error: "canceled",
  });
  flushSftpTransferFrames();

  assert.equal(api.transfers.value[0].status, "canceled");
  assert.equal(api.transfers.value[0].transferred, 4);
  assert.equal(api.transfers.value[0].progress, 40);
  assert.equal(api.transfers.value[0].error, "");
});

test("目录同名也询问冲突，同批 overwrite 重复目标拒绝启动并 toast", async () => {
  const conflicts = [];
  const { api } = await mountTransfers({
    remoteFileByName: ref(new Map([["same", { name: "same", path: "/remote/same", kind: "dir" }]])),
    requestUploadConflictAction: async (conflict) => {
      conflicts.push(conflict);
      return "overwrite";
    },
  });

  await api.uploadLocalPaths(["/first/same", "/second/same"]);

  assert.equal(conflicts.length, 2);
  assert.ok(conflicts.every((conflict) => conflict.entry.kind === "dir"));
  assert.equal(calls("transferSftp").length, 1);
  assert.equal(calls("transferSftp")[0].args[0].uploadConflictAction, "overwrite");
  assert.equal(api.transfers.value.length, 1);
  assert.ok(
    sftpTransfersTestState.toasts.some(
      (toast) => toast.type === "error" && toast.message.includes("same"),
    ),
  );
});

test("下载和上传选择器 reject 均转化为 toast", async () => {
  const { api } = await mountTransfers();
  mockSftpState.handlers.chooseSftpDownloadPath = () =>
    Promise.reject(new Error("download dialog rejected"));
  mockSftpState.handlers.chooseSftpUploadFiles = () =>
    Promise.reject(new Error("upload dialog rejected"));

  await api.startDownload(entry("dialog.txt"));
  await api.chooseAndUpload();

  assert.equal(calls("transferSftp").length, 0);
  assert.equal(sftpTransfersTestState.toasts.length, 2);
  assert.match(sftpTransfersTestState.toasts[0].message, /download dialog rejected/);
  assert.match(sftpTransfersTestState.toasts[1].message, /upload dialog rejected/);
});

test("卸载后忽略 deferred list 并注销进度监听", async () => {
  const list = deferred();
  mockSftpState.handlers.listSftpTransfers = () => list.promise;
  const { api, app } = await mountTransfers();
  assert.equal(sftpTransfersTestState.listeners.size, 1);

  app.unmount();
  mountedApps.delete(app);
  list.resolve([backendItem("late")]);
  await settle();

  assert.equal(sftpTransfersTestState.listeners.size, 0);
  assert.equal(api.transfers.value.length, 0);
});

test("恢复列表期间的 100 个未知终态不会被缓冲上限淘汰", async () => {
  const list = deferred();
  mockSftpState.handlers.listSftpTransfers = () => list.promise;
  const { api } = await mountTransfers();
  const payloads = Array.from({ length: 100 }, (_, index) => ({
    transferId: `restored-${index}`,
    transferred: 10,
    total: 10,
    done: true,
    status: "done",
    error: null,
  }));

  for (const payload of payloads) emitSftpTransferProgress(payload);
  flushSftpTransferFrames();
  list.resolve(payloads.map((payload) => backendItem(payload.transferId, { transferred: 2 })));
  await settle();

  assert.equal(api.transfers.value.length, 50);
  assert.ok(api.transfers.value.every((item) => item.status === "done"));
  assert.equal(api.activeTransfers.value, 0);
});

test("列表期间新任务的同字节暂停事件不会被旧 running 快照覆盖", async () => {
  const { api, props } = await mountTransfers();
  const list = deferred();
  mockSftpState.handlers.listSftpTransfers = () => list.promise;
  props.visible = true;
  await nextTick();
  const item = await startItem(api, "new-paused.txt");

  emitSftpTransferProgress({
    transferId: item.id,
    transferred: 0,
    total: 10,
    done: false,
    status: "paused",
    error: null,
  });
  flushSftpTransferFrames();
  list.resolve([backendItem(item.id, { transferred: 0, status: "running" })]);
  await settle();

  assert.equal(api.transfers.value[0].status, "paused");
  assert.equal(api.transfers.value[0].speed, "sftp.paused");
});

test("同批 resume 的重复目标也只启动一个传输", async () => {
  const { api } = await mountTransfers({
    remoteFileByName: ref(
      new Map([["same.txt", { name: "same.txt", path: "/remote/same.txt", kind: "file" }]]),
    ),
    requestUploadConflictAction: async () => "resume",
  });

  await api.uploadLocalPaths(["/first/same.txt", "/second/same.txt"]);

  assert.equal(calls("transferSftp").length, 1);
  assert.equal(api.transfers.value.length, 1);
  assert.ok(
    sftpTransfersTestState.toasts.some((toast) =>
      String(toast.message).includes("uploadDuplicateTarget"),
    ),
  );
});

test("下载选择框等待期间重连不会启动旧条目", async () => {
  const dialog = deferred();
  mockSftpState.handlers.chooseSftpDownloadPath = () => dialog.promise;
  const { api, props } = await mountTransfers();

  const downloading = api.startDownload(entry("old.txt"));
  props.sessionId = "s2";
  dialog.resolve("/downloads/old.txt");
  await downloading;

  assert.equal(calls("transferSftp").length, 0);
  assert.equal(api.transfers.value.length, 0);
});

test("上传冲突弹窗等待期间重连不会写入新会话", async () => {
  const conflict = deferred();
  const { api, props } = await mountTransfers({
    remoteFileByName: ref(
      new Map([["same.txt", { name: "same.txt", path: "/remote/same.txt", kind: "file" }]]),
    ),
    requestUploadConflictAction: () => conflict.promise,
  });

  const uploading = api.uploadLocalPaths(["/local/same.txt"]);
  await settle();
  props.sessionId = "s2";
  conflict.resolve("overwrite");
  await uploading;

  assert.equal(calls("transferSftp").length, 0);
  assert.equal(api.transfers.value.length, 0);
});

test("上传选择框保留发起时的目标目录", async () => {
  const dialog = deferred();
  mockSftpState.handlers.chooseSftpUploadFiles = () => dialog.promise;
  const { api, remotePath } = await mountTransfers();

  const choosing = api.chooseAndUpload();
  remotePath.value = "/other";
  dialog.resolve(["/local/a.txt"]);
  await choosing;

  assert.equal(calls("transferSftp")[0].args[0].remoteParentPath, "/remote");
});

test("恢复 queued 记录保持活动、可取消且不会被终态清理", async () => {
  mockSftpState.handlers.listSftpTransfers = () => [backendItem("queued", { status: "queued" })];
  const { api } = await mountTransfers();

  assert.equal(api.activeTransfers.value, 1);
  assert.equal(api.transfers.value[0].speed, "sftp.queued");
  api.clearCompletedTransfers();
  assert.equal(api.transfers.value.length, 1);

  await api.cancelTransfer("queued");
  assert.equal(calls("cancelSftpTransfer").length, 1);
  assert.equal(api.transfers.value[0].status, "queued");
});
