import assert from "node:assert/strict";
import { register } from "node:module";
import test, { mock } from "node:test";

const motionUrl = new URL("../src/utils/motion/index.js", import.meta.url).href;
const resolver = `
  export async function resolve(specifier, context, nextResolve) {
    if (specifier.endsWith("/utils/motion")) {
      return { url: ${JSON.stringify(motionUrl)}, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  }
`;
register(`data:text/javascript,${encodeURIComponent(resolver)}`, import.meta.url);

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
globalThis.window = {
  matchMedia: () => ({ matches: false, addEventListener() {} }),
};
globalThis.document = {
  documentElement: { dataset: {} },
  createElement: () => ({}),
};

const { createRenderer, nextTick, ref } = await import("vue");
const { useSftpMotion } = await import("../src/composables/useSftpMotion.js");

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

const mountedApps = [];

function createRow(path) {
  return {
    dataset: { path, rowKey: path },
    animate: mock.fn(() => {
      const listeners = new Map();
      return {
        addEventListener(type, callback) {
          listeners.set(type, callback);
        },
        cancel: mock.fn(() => listeners.get("cancel")?.()),
      };
    }),
  };
}

function mountMotion(options) {
  const app = renderer.createApp({
    setup() {
      useSftpMotion(options);
      return () => null;
    },
  });
  app.mount({});
  mountedApps.push(app);
}

async function settle() {
  await nextTick();
  await new Promise((resolve) => setImmediate(resolve));
}

test.afterEach(() => {
  for (const app of mountedApps.splice(0)) app.unmount();
});

test.after(() => {
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
});

test("虚拟滚动切换可见行后不为已有数据重播进入动画", async () => {
  const files = Array.from({ length: 100 }, (_, index) => ({
    name: `file-${index}`,
    path: `/remote/file-${index}`,
  }));
  const filteredRemoteFiles = ref([]);
  const loading = ref(false);
  let currentRows = [];
  const tableBodyRef = ref({ querySelectorAll: () => currentRows });
  mountMotion({
    tableBodyRef,
    queueListRef: ref(null),
    filteredRemoteFiles,
    loading,
    transfers: ref([]),
  });

  const firstPage = files.slice(0, 10).map((entry) => createRow(entry.path));
  currentRows = firstPage;
  filteredRemoteFiles.value = files;
  await settle();
  for (const row of firstPage) assert.equal(row.animate.mock.calls.length, 1);

  const secondPage = files.slice(10, 20).map((entry) => createRow(entry.path));
  currentRows = secondPage;
  filteredRemoteFiles.value = files.map((entry) => ({ ...entry }));
  await settle();

  for (const row of secondPage) assert.equal(row.animate.mock.calls.length, 0);
});
