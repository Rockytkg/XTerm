import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { createRenderer, nextTick } = await import("vue");
const { mockSftpState, resetMockSftpState } = await import("./helpers/mockSftpService.mjs");
const { useSftpBrowser } = await import("../src/composables/useSftpBrowser.js");

const t = (key) => key;
const apps = [];
let previousWindow;

const renderer = createRenderer({
  patchProp() {},
  insert() {},
  remove() {},
  createElement: (type) => ({ type }),
  createText: (text) => ({ text }),
  createComment: (text) => ({ text }),
  setText() {},
  setElementText() {},
  parentNode: () => null,
  nextSibling: () => null,
  querySelector: () => null,
  setScopeId() {},
  cloneNode: (node) => ({ ...node }),
  insertStaticContent: () => [null, null],
});

function createProps(overrides = {}) {
  return {
    connection: { id: "c1", capabilities: { sftp: true } },
    sessionId: "s1",
    workingDirectory: "/initial",
    visible: true,
    ...overrides,
  };
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function settle() {
  await nextTick();
  await new Promise((resolve) => setImmediate(resolve));
}

async function mountBrowser(props = createProps(), { waitForMount = true } = {}) {
  let api;
  const app = renderer.createApp({
    setup() {
      api = useSftpBrowser({ props, t });
      return () => null;
    },
  });
  app.mount({});
  apps.push(app);
  if (waitForMount) await settle();
  return api;
}

function listPaths() {
  return mockSftpState.calls
    .filter((call) => call.name === "listRemoteSftp")
    .map((call) => call.args[2]);
}

test.beforeEach(() => {
  resetMockSftpState();
  previousWindow = globalThis.window;
  globalThis.window = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
  };
  mockSftpState.handlers.listRemoteSftp = async (_connectionId, _sessionId, path) => ({
    entries: [],
    parent: null,
    path,
  });
});

test.afterEach(() => {
  while (apps.length) apps.pop().unmount();
  if (previousWindow === undefined) {
    delete globalThis.window;
  } else {
    globalThis.window = previousWindow;
  }
});

test("最新增量请求与过期普通请求结束后都清理加载状态", async () => {
  const first = deferred();
  const second = deferred();
  mockSftpState.handlers.listRemoteSftp = (_connectionId, _sessionId, path) => {
    if (path === "/first") return first.promise;
    if (path === "/second") return second.promise;
    return { entries: [], parent: null, path };
  };
  const api = await mountBrowser();

  const firstRequest = api.refreshRemote("/first", { pathLoading: true });
  const secondRequest = api.refreshRemote("/second", {
    incremental: true,
    preserveEntries: true,
  });
  assert.equal(api.loading.value, false);
  assert.equal(api.pathLoading.value, false);

  second.resolve({ entries: [], parent: null, path: "/second" });
  assert.equal(await secondRequest, true);
  assert.equal(api.loading.value, false);
  assert.equal(api.pathLoading.value, false);

  first.resolve({ entries: [], parent: null, path: "/first" });
  assert.equal(await firstRequest, null);
  assert.equal(api.remotePath.value, "/second");
  assert.equal(api.loading.value, false);
  assert.equal(api.pathLoading.value, false);
});

test("初始化请求被新导航取代后不再回退到点目录", async () => {
  const initial = deferred();
  const next = deferred();
  mockSftpState.handlers.listRemoteSftp = (_connectionId, _sessionId, path) => {
    if (path === "/start") return initial.promise;
    if (path === "/next") return next.promise;
    return { entries: [], parent: null, path };
  };
  const api = await mountBrowser(createProps({ workingDirectory: "/start" }), {
    waitForMount: false,
  });

  const navigation = api.refreshRemote("/next");
  next.resolve({ entries: [], parent: null, path: "/next" });
  assert.equal(await navigation, true);
  initial.resolve({ entries: [], parent: null, path: "/start" });
  await settle();

  assert.deepEqual(listPaths(), ["/start", "/next"]);
  assert.equal(api.remotePath.value, "/next");
});

test("初始化真实失败仍会回退到点目录", async () => {
  mockSftpState.handlers.listRemoteSftp = async (_connectionId, _sessionId, path) => {
    if (path === "/missing") throw new Error("missing");
    return { entries: [], parent: null, path: "/" };
  };
  const api = await mountBrowser(createProps({ workingDirectory: "/missing" }));

  assert.deepEqual(listPaths(), ["/missing", "."]);
  assert.equal(api.remotePath.value, "/");
  assert.equal(api.loading.value, false);
  assert.equal(api.pathLoading.value, false);
});

test("批量删除部分失败后关闭旧确认并刷新真实剩余项", async () => {
  const entries = [
    { name: "a.txt", path: "/initial/a.txt", kind: "file", size: 1, modified: 1 },
    { name: "b.txt", path: "/initial/b.txt", kind: "file", size: 1, modified: 1 },
  ];
  mockSftpState.handlers.listRemoteSftp = async () => ({
    entries,
    parent: "/",
    path: "/initial",
  });
  const api = await mountBrowser();
  mockSftpState.handlers.deleteRemoteSftp = async () => {
    throw new Error("b.txt: permission denied");
  };
  mockSftpState.handlers.listRemoteSftp = async () => ({
    entries: [entries[1]],
    parent: "/",
    path: "/initial",
  });

  api.requestDeleteEntries(api.remoteFiles.value);
  await api.confirmDeleteEntries();

  assert.equal(listPaths().length, 2);
  assert.equal(api.deleteDialog.value.open, false);
  assert.equal(api.deleteDialog.value.deleting, false);
  assert.equal(api.errorMessage.value, "b.txt: permission denied");
  assert.deepEqual(
    api.remoteFiles.value.map((entry) => entry.name),
    ["b.txt"],
  );
});

test("用户新建成功后的目录刷新失败会显示错误", async () => {
  const api = await mountBrowser();
  mockSftpState.handlers.listRemoteSftp = async () => {
    throw new Error("list failed");
  };
  api.startCreateFile();
  api.inlineEdit.value = { ...api.inlineEdit.value, value: "new.txt" };
  await api.commitInlineEdit();

  assert.equal(
    mockSftpState.calls.some((call) => call.name === "createRemoteSftpFile"),
    true,
  );
  assert.equal(api.errorMessage.value, "list failed");
});

test("路径编辑提交与随后的失焦不会重复导航", async () => {
  const api = await mountBrowser();
  mockSftpState.calls.length = 0;
  api.startPathEdit();
  api.editablePath.value = "/next";

  api.submitPathEdit();
  api.submitPathEdit();
  await settle();

  assert.deepEqual(listPaths(), ["/next"]);
  assert.equal(api.editingPath.value, false);
});

test("原名带首尾空格时未修改的重命名不会发 IPC", async () => {
  const entry = {
    name: "report ",
    path: "/initial/report ",
    kind: "file",
    size: 1,
    modified: 1,
  };
  mockSftpState.handlers.listRemoteSftp = async () => ({
    entries: [entry],
    parent: "/",
    path: "/initial",
  });
  const api = await mountBrowser();
  mockSftpState.calls.length = 0;

  api.startRenameEntry(entry);
  await api.commitInlineEdit();

  assert.equal(mockSftpState.calls.length, 0);
  assert.equal(api.inlineEdit.value.active, false);
});

test("字面 tilde 目录的面包屑持续保留相对前缀", async () => {
  mockSftpState.handlers.listRemoteSftp = async () => ({
    entries: [],
    parent: "./~",
    path: "./~/child",
  });
  const api = await mountBrowser();

  assert.deepEqual(api.pathCrumbs.value, [
    { label: ".", path: "." },
    { label: "~", path: "./~" },
    { label: "child", path: "./~/child" },
  ]);
  assert.equal(api.remoteParent.value, "./~");
});
