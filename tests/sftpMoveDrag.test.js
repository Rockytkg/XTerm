import assert from "node:assert/strict";
import { register } from "node:module";
import test, { mock } from "node:test";

const sftpMockUrl = `data:text/javascript,${encodeURIComponent(`
  export const mockSftpState = { calls: [] };
  export function resetMockSftpState() {
    mockSftpState.calls.length = 0;
  }
  export async function listRemoteSftp() {
    return { entries: [] };
  }
  export async function renameRemoteSftp(...args) {
    mockSftpState.calls.push({ name: "renameRemoteSftp", args });
  }
`)}`;
const resolver = `
  export async function resolve(specifier, context, nextResolve) {
    if (specifier.endsWith("/services/sftp") || specifier.endsWith("/services/sftp.js")) {
      return { url: ${JSON.stringify(sftpMockUrl)}, shortCircuit: true };
    }
    try {
      return await nextResolve(specifier, context);
    } catch (error) {
      if (error?.code === "ERR_MODULE_NOT_FOUND" && specifier.startsWith(".")) {
        return nextResolve(specifier + ".js", context);
      }
      throw error;
    }
  }
`;
register(`data:text/javascript,${encodeURIComponent(resolver)}`, import.meta.url);

const { createRenderer, ref } = await import("vue");
const { mockSftpState, resetMockSftpState } = await import(sftpMockUrl);
const { useSftpMoveDrag } = await import("../src/composables/useSftpMoveDrag.js");

const renderer = createRenderer({
  patchProp() {},
  insert() {},
  remove() {},
  createElement: () => ({}),
  createText: () => ({}),
  createComment: () => ({}),
  setText() {},
  setElementText() {},
  parentNode: () => null,
  nextSibling: () => null,
  querySelector: () => null,
  setScopeId() {},
  insertStaticContent: () => [null, null],
});

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
const mountedApps = [];
let browser;
let insideBrowserElement;
let pointResolver;
let windowStub;

function createClassList(initial = []) {
  const classes = new Set(initial);
  return {
    add: (...names) => names.forEach((name) => classes.add(name)),
    remove: (...names) => names.forEach((name) => classes.delete(name)),
    contains: (name) => classes.has(name),
  };
}

function createWindowStub() {
  let nextTimerId = 1;
  let nextFrameId = 1;
  const listeners = new Map();
  const timers = new Map();
  const frames = new Map();
  return {
    timers,
    frames,
    setTimeout(callback) {
      const id = nextTimerId++;
      timers.set(id, callback);
      return id;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
    requestAnimationFrame(callback) {
      const id = nextFrameId++;
      frames.set(id, callback);
      return id;
    },
    cancelAnimationFrame(id) {
      frames.delete(id);
    },
    addEventListener(type, callback) {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type).add(callback);
    },
    removeEventListener(type, callback) {
      listeners.get(type)?.delete(callback);
    },
    dispatch(type, event) {
      for (const callback of [...(listeners.get(type) || [])]) callback(event);
    },
    runTimer(id) {
      const callback = timers.get(id);
      assert.ok(callback, `missing timer ${id}`);
      timers.delete(id);
      callback();
    },
  };
}

function createDocumentStub() {
  return {
    body: {
      classList: createClassList(),
      appendChild(element) {
        element.parentNode = this;
      },
    },
    elementFromPoint: (x, y) => pointResolver(x, y),
    createElement(tagName) {
      return {
        tagName,
        className: "",
        textContent: "",
        style: {},
        children: [],
        append(...children) {
          this.children.push(...children);
        },
        remove() {
          this.removed = true;
        },
      };
    },
  };
}

function directoryRow(path) {
  const row = {
    dataset: { path },
    classList: createClassList(["is-dir"]),
    closest(selector) {
      if (selector === ".sftp-browser") return browser;
      if (selector === ".sftp-row.is-dir[data-path]") return row;
      return null;
    },
  };
  return row;
}

function sourceRow(entry) {
  const row = {
    dataset: { path: entry.path },
    classList: createClassList(["sftp-move-draggable"]),
    focus: mock.fn(),
    getBoundingClientRect: () => ({ width: 320 }),
    closest(selector) {
      if (selector === ".sftp-move-draggable") return row;
      if (selector === ".sftp-browser") return browser;
      return null;
    },
  };
  return row;
}

function mouseEvent({ target = null, button = 0, clientX = 0, clientY = 0 } = {}) {
  return {
    target,
    button,
    clientX,
    clientY,
    defaultPrevented: false,
    propagationStopped: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
    stopPropagation() {
      this.propagationStopped = true;
    },
  };
}

function mountMoveDrag(options) {
  let api;
  const app = renderer.createApp({
    setup() {
      api = useSftpMoveDrag(options);
      return () => null;
    },
  });
  app.mount({});
  mountedApps.push(app);
  return api;
}

function createHarness() {
  const entry = { name: "file.txt", path: "/a/file.txt", kind: "file", size: 1 };
  const row = sourceRow(entry);
  const props = { visible: true, connection: { id: "c1" }, sessionId: "s1" };
  const remoteFileMapForPath = mock.fn(async () => new Map());
  const options = {
    props,
    remotePath: ref("/a"),
    remoteParent: ref("/"),
    remoteFiles: ref([entry]),
    remoteFileByName: ref(new Map()),
    remoteFileMapForPath,
    refreshCurrentDirectoryIncremental: mock.fn(async () => undefined),
    selectedNames: ref(new Set()),
    inlineEdit: ref({ active: false }),
    loading: ref(false),
    errorMessage: ref(""),
    refreshRemote: mock.fn(async () => true),
    closeContextMenu: mock.fn(),
    t: (key) => key,
  };
  const api = mountMoveDrag(options);
  api.tableBodyRef.value = { contains: (candidate) => candidate === row };
  return { api, entry, props, remoteFileMapForPath, row };
}

function beginDrag(api, row) {
  const down = mouseEvent({ target: row, clientX: 1, clientY: 1 });
  api.onMoveMouseDown(down);
  assert.equal(windowStub.timers.size, 1);
  windowStub.runTimer(windowStub.timers.keys().next().value);
  assert.equal(api.moveDragActive.value, true);
  return down;
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

test.beforeEach(() => {
  resetMockSftpState();
  windowStub = createWindowStub();
  browser = { contains: () => true };
  insideBrowserElement = {
    closest: (selector) => (selector === ".sftp-browser" ? browser : null),
  };
  pointResolver = () => insideBrowserElement;
  globalThis.window = windowStub;
  globalThis.document = createDocumentStub();
});

test.afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount();
});

test.after(() => {
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
});

test("mouseup 在 flush 前采用最终坐标提交并抑制同次合成 click", async () => {
  const { api, remoteFileMapForPath, row } = createHarness();
  const staleTarget = directoryRow("/stale");
  const finalTarget = directoryRow("/final");
  pointResolver = (x) => (x === 10 ? staleTarget : finalTarget);
  beginDrag(api, row);

  windowStub.dispatch("mousemove", mouseEvent({ clientX: 10, clientY: 10 }));
  assert.equal(windowStub.frames.size, 1);
  windowStub.dispatch("mouseup", mouseEvent({ clientX: 20, clientY: 20 }));
  await settle();

  assert.equal(windowStub.frames.size, 0);
  assert.equal(remoteFileMapForPath.mock.calls[0].arguments[0], "/final");
  const rename = mockSftpState.calls.find((call) => call.name === "renameRemoteSftp");
  assert.deepEqual(rename.args, ["c1", "s1", "/a/file.txt", "/final", "file.txt", "create"]);

  const click = mouseEvent();
  api.suppressMoveClick(click);
  assert.equal(click.defaultPrevented, true);
  assert.equal(click.propagationStopped, true);
  const followingClick = mouseEvent();
  api.suppressMoveClick(followingClick);
  assert.equal(followingClick.defaultPrevented, false);
});

test("长按等待期间重连不会激活旧条目的拖动", () => {
  const { api, props, row } = createHarness();
  api.onMoveMouseDown(mouseEvent({ target: row, clientX: 1, clientY: 1 }));
  assert.equal(windowStub.timers.size, 1);

  props.sessionId = "s2";
  windowStub.runTimer(windowStub.timers.keys().next().value);

  assert.equal(api.moveDragActive.value, false);
  assert.equal(windowStub.timers.size, 0);
  assert.equal(
    mockSftpState.calls.some((call) => call.name === "renameRemoteSftp"),
    false,
  );
});

test("拖放开始后的重连不会把旧移动提交到新 session", async () => {
  const { api, props, remoteFileMapForPath, row } = createHarness();
  pointResolver = () => directoryRow("/final");
  beginDrag(api, row);

  props.sessionId = "s2";
  windowStub.dispatch("mouseup", mouseEvent({ clientX: 20, clientY: 20 }));
  await settle();

  assert.equal(remoteFileMapForPath.mock.calls.length, 0);
  assert.equal(
    mockSftpState.calls.some((call) => call.name === "renameRemoteSftp"),
    false,
  );
});

test("cancelDrag 清理残留 suppressClick", () => {
  const { api, row } = createHarness();
  beginDrag(api, row);
  windowStub.dispatch("blur", {});

  assert.equal(api.moveDragActive.value, false);
  const click = mouseEvent();
  api.suppressMoveClick(click);
  assert.equal(click.defaultPrevented, false);
  assert.equal(click.propagationStopped, false);
});

test("新左键按下保留 preventDefault、聚焦行并清除残留 suppressClick", async () => {
  const { api, row } = createHarness();
  const firstDown = beginDrag(api, row);
  assert.equal(firstDown.defaultPrevented, true);
  windowStub.dispatch("mouseup", mouseEvent({ clientX: 2, clientY: 2 }));
  await settle();

  const secondDown = mouseEvent({ target: row, clientX: 3, clientY: 3 });
  api.onMoveMouseDown(secondDown);
  assert.equal(secondDown.defaultPrevented, true);
  assert.deepEqual(row.focus.mock.calls.at(-1).arguments, [{ preventScroll: true }]);

  const click = mouseEvent();
  api.suppressMoveClick(click);
  assert.equal(click.defaultPrevented, false);
  assert.equal(click.propagationStopped, false);
});
