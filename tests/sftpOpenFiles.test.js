// useSftpOpenFiles 单文件打开模型测试。service 层经 loader 替换为内存桩（见 helpers/），
// 覆盖打开即加载完成、打开即替换的核心语义——也是 ref 深响应式代理导致
// 对象同一性守卫（openFile.value === tab）失效、加载结果全部被丢弃的回归防护。
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { createSSRApp, nextTick, reactive } = await import("vue");
const { compileScript, parse } = await import("vue/compiler-sfc");
const { renderToString } = await import("vue/server-renderer");
const { createI18n } = await import("vue-i18n");
const { mockSftpState, resetMockSftpState, saveSftpPreviewResource } =
  await import("./helpers/mockSftpService.mjs");
const { PREVIEW_BINARY_MAX_BYTES, PREVIEW_TEXT_MAX_BYTES } =
  await import("../src/utils/filePreview.js");
const { useSftpOpenFiles } = await import("../src/composables/useSftpOpenFiles.js");

const props = {
  connection: { id: "c1", capabilities: { sftp: true } },
  sessionId: "s1",
};
const t = (key) => key;

test.beforeEach(() => {
  resetMockSftpState();
});

function createApi(overrides = {}) {
  return useSftpOpenFiles({ props, t, ...overrides });
}

function fileEntry(name) {
  return { name, path: `/home/${name}`, size: 5, kind: "file" };
}

function createDeferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function serviceCalls(name) {
  return mockSftpState.calls.filter((call) => call.name === name);
}

function dataModule(source) {
  return `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
}

const viewerPluginNames = [
  "archivePlugin",
  "audioPlugin",
  "cadPlugin",
  "drawingPlugin",
  "emailPlugin",
  "fallbackPlugin",
  "imagePlugin",
  "model3dPlugin",
  "officePlugin",
  "pdfPlugin",
  "textPlugin",
  "videoPlugin",
  "xmindPlugin",
];
const sfcImportReplacements = {
  vue: import.meta.resolve("vue"),
  "vue-i18n": import.meta.resolve("vue-i18n"),
  "@lucide/vue": dataModule("export const ArrowLeft = {};"),
  "@open-file-viewer/core": dataModule(
    viewerPluginNames.map((name) => `export const ${name} = () => ({});`).join("\n"),
  ),
  "@open-file-viewer/vue": dataModule("export const OpenFileViewer = {};"),
  "pdfjs-dist/build/pdf.worker.mjs?url": dataModule('export default "pdf-worker";'),
  "../../utils/filePreview": new URL("../src/utils/filePreview.js", import.meta.url).href,
  "../../utils/formatBytes": new URL("../src/utils/formatBytes.js", import.meta.url).href,
  "../../utils/logger": dataModule("export const createLogger = () => ({ warn() {} });"),
};
const sfcModuleCache = new Map();

async function loadSfc(path) {
  if (sfcModuleCache.has(path.href)) return sfcModuleCache.get(path.href);
  const loading = (async () => {
    const source = await readFile(path, "utf8");
    const { descriptor } = parse(source, { filename: path.href });
    let compiled = compileScript(descriptor, { id: "sftp-test", inlineTemplate: false }).content;
    compiled = compiled.replace(/^import\s+["'][^"']+\.(?:css|scss)["'];?\s*$/gm, "");
    for (const [specifier, replacement] of Object.entries(sfcImportReplacements)) {
      compiled = compiled.replaceAll(`"${specifier}"`, JSON.stringify(replacement));
    }
    return import(dataModule(compiled));
  })();
  sfcModuleCache.set(path.href, loading);
  return loading;
}

async function setupSfc(path, props) {
  const { default: component } = await loadSfc(path);
  let bindings;
  const app = createSSRApp({
    setup() {
      bindings = component.setup(props, {
        attrs: {},
        emit() {},
        expose() {},
        slots: {},
      });
      return () => null;
    },
  });
  app.use(
    createI18n({
      legacy: false,
      locale: "en-US",
      messages: {
        "en-US": {
          sftp: {
            chooseDownloadTitle: "Save resource",
            preview: { saveFailed: "Resource save failed" },
          },
        },
      },
    }),
  );
  await renderToString(app);
  return bindings;
}

function openFileViewProps() {
  const noop = () => {};
  return {
    closeFile: noop,
    convertToEdit: noop,
    downloadFile: noop,
    downloadResource: ({ name, contentBase64 }) =>
      saveSftpPreviewResource({
        defaultFileName: name,
        contentBase64,
        title: "Save resource",
      }),
    file: null,
    fontSizeChange: noop,
    preferences: {},
    saveFile: noop,
    saveFileAndClose: noop,
    updateContent: noop,
  };
}

function downloadAnchor(name, attachment = true) {
  const anchor = {
    href: "blob:attachment",
    getAttribute: () => name,
    closest: (selector) =>
      attachment && selector === ".ofv-email-attachment-item" ? anchor : null,
  };
  return anchor;
}

test("openPreview 对已知文本保持预览并标记可转编辑", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("a.txt")]);
  const file = api.openFile.value;
  assert.equal(file.mode, "preview");
  assert.equal(file.editable, true);
  assert.ok(file.blob instanceof Blob);
  assert.equal(file.blob.type, "text/plain");
  assert.equal(file.loading, false);
  assert.equal(file.error, "");
  assert.equal(mockSftpState.statCalls, 1);
  assert.equal(mockSftpState.readCalls, 1);
});

test("openPreview 通过内容嗅探识别无扩展名文本并保持预览", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("README")]);
  const file = api.openFile.value;
  assert.equal(file.mode, "preview");
  assert.equal(file.editable, true);
  assert.ok(file.blob instanceof Blob);
  assert.equal(file.mime, "text/plain");
});

test("文本预览经 convertToEdit 转入编辑", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("a.txt")]);
  assert.equal(api.openFile.value.mode, "preview");
  assert.equal(await api.convertToEdit(), true);
  const file = api.openFile.value;
  assert.equal(file.mode, "edit");
  assert.equal(file.loading, false);
  assert.equal(file.content, "hello text");
  assert.equal(file.savedContent, "hello text");
});

test("openEditor 完成加载并写入文本内容", async () => {
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));
  const file = api.openFile.value;
  assert.equal(file.mode, "edit");
  assert.equal(file.loading, false);
  assert.equal(file.content, "hello text");
  assert.equal(file.savedContent, "hello text");
});

test("再次打开替换当前文件（无脏检查阻塞时）", async () => {
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));
  await api.openPreview([fileEntry("c.png")]);
  assert.equal(api.openFile.value.path, "/home/c.png");
  assert.equal(api.openFile.value.mode, "preview");
});

test("closeEditor 清空当前文件回到列表", async () => {
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));
  assert.equal(await api.closeEditor(), true);
  assert.equal(api.openFile.value, null);
});

// 回归：saveEditor/convertToEdit 自身先 patchTab 置位（saving/loading）会替换 openFile，
// 之后若仍按对象同一性守卫，保存与转编辑会永远静默失败。
test("saveEditor 保存成功并更新已保存内容", async () => {
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));
  api.updateEditorContent("changed content");
  assert.equal(await api.saveEditor(), true);
  const file = api.openFile.value;
  assert.equal(file.savedContent, "changed content");
  assert.equal(file.saving, false);
  assert.equal(file.error, "");
  assert.equal(file.modified, 1700000001);
});

// 回归：saveEditor 经 patchTab 整体替换 openFile（新对象 !== tab），
// closeEditor 若按对象同一性判断，脏检查选"保存"后文件会关不掉。
test("closeEditor 脏文件选择保存后正常关闭", async () => {
  const api = createApi({ requestDirtyAction: async () => "save" });
  await api.openEditor(fileEntry("b.txt"));
  api.updateEditorContent("changed content");
  assert.equal(await api.closeEditor(), true);
  assert.equal(api.openFile.value, null);
});

// 回归：重连（sessionId 变化）后旧会话的打开文件必须丢弃——保留会让保存经
// currentSession() 写入新会话的同路径文件（跨会话写）。
test("会话切换后打开的文件被丢弃", async () => {
  const reactiveProps = reactive({
    connection: { id: "c1", capabilities: { sftp: true } },
    sessionId: "s1",
  });
  const api = useSftpOpenFiles({ props: reactiveProps, t });
  await api.openEditor(fileEntry("b.txt"));
  assert.ok(api.openFile.value);
  reactiveProps.sessionId = "s2";
  await nextTick();
  assert.equal(api.openFile.value, null);
});

// 回归：脏检查弹窗等待期间会话被切换时，openPreview 不得把后续文件挂到旧会话上
// （会话 watch 清空 openFile 后，replace 会让 tab 永远停在加载态）。
test("openPreview 多文件时会话在脏检查期间切换则中止", async () => {
  const reactiveProps = reactive({
    connection: { id: "c1", capabilities: { sftp: true } },
    sessionId: "s1",
  });
  const api = useSftpOpenFiles({
    props: reactiveProps,
    t,
    requestDirtyAction: async () => {
      reactiveProps.sessionId = "s2";
      return "discard";
    },
  });
  await api.openEditor(fileEntry("b.txt"));
  api.updateEditorContent("dirty content");
  await api.openPreview([fileEntry("c.png"), fileEntry("d.png")]);
  await nextTick();
  assert.equal(api.openFile.value, null);
});

// 回归：stat 的 modified 为 0（epoch）是合法值，|| 回退会错误采用列表里的旧时间戳。
test("stat 的 modified 为 0 时不回退到列表时间戳", async () => {
  mockSftpState.stat = { size: 5, modified: 0, mime: "text/plain" };
  try {
    const api = createApi();
    await api.openEditor({ ...fileEntry("b.txt"), modified: 1700000123 });
    assert.equal(api.openFile.value.modified, 0);
    assert.equal(api.openFile.value.mtime, 0);
  } finally {
    resetMockSftpState();
  }
});

test("openEditor 读取失败后内容未加载且禁止保存", async () => {
  mockSftpState.handlers.readRemoteSftpFile = async () => {
    throw new Error("read denied");
  };
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));

  const file = api.openFile.value;
  assert.equal(file.contentLoaded, false);
  assert.equal(file.loading, false);
  assert.match(file.error, /read denied/);
  assert.equal(await api.saveEditor(), false);
  assert.equal(serviceCalls("writeRemoteSftpFile").length, 0);
  api.updateEditorContent("must not be accepted");
  assert.equal(api.openFile.value.content, "");
});

test("保存失败不破坏已加载状态并可重试", async () => {
  const api = createApi();
  await api.openEditor(fileEntry("b.txt"));
  api.updateEditorContent("changed content");
  let attempts = 0;
  mockSftpState.handlers.writeRemoteSftpFile = async () => {
    attempts += 1;
    if (attempts === 1) throw new Error("write denied");
    return { modified: 1700000002 };
  };

  assert.equal(await api.saveEditor(), false);
  assert.equal(api.openFile.value.contentLoaded, true);
  assert.equal(api.openFile.value.saving, false);
  assert.match(api.openFile.value.error, /write denied/);
  assert.equal(await api.saveEditor(), true);
  assert.equal(attempts, 2);
  assert.equal(api.openFile.value.savedContent, "changed content");
  assert.equal(api.openFile.value.error, "");
});

test("openEditor 无已知 stat 时严格先 stat 再 read", async () => {
  const statPending = createDeferred();
  const statStarted = createDeferred();
  mockSftpState.handlers.statRemoteSftpFile = () => {
    statStarted.resolve();
    return statPending.promise;
  };
  const api = createApi();
  const opening = api.openEditor(fileEntry("b.txt"));
  await statStarted.promise;

  assert.equal(mockSftpState.readCalls, 0);
  assert.deepEqual(
    mockSftpState.calls.map((call) => call.name),
    ["statRemoteSftpFile"],
  );
  statPending.resolve({ size: 5, modified: 1700000000, mime: "text/plain" });
  await opening;
  assert.deepEqual(
    mockSftpState.calls.map((call) => call.name),
    ["statRemoteSftpFile", "readRemoteSftpFile"],
  );
});

test("openPreview 多选只打开最后一个有效文件", async () => {
  const api = createApi();
  await api.openPreview([
    fileEntry("first.txt"),
    { name: "directory", path: "/home/directory", kind: "dir" },
    fileEntry("last.txt"),
  ]);

  assert.equal(api.openFile.value.path, "/home/last.txt");
  assert.equal(mockSftpState.statCalls, 1);
  assert.equal(mockSftpState.readCalls, 1);
  assert.deepEqual(
    mockSftpState.calls.map((call) => call.name),
    ["statRemoteSftpFile", "readRemoteSftpFile"],
  );
  assert.ok(mockSftpState.calls.every((call) => call.args[2] === "/home/last.txt"));
});

test("关闭加载中的预览后不再读取或重新打开", async () => {
  const statPending = createDeferred();
  const statStarted = createDeferred();
  mockSftpState.handlers.statRemoteSftpFile = () => {
    statStarted.resolve();
    return statPending.promise;
  };
  const api = createApi();
  const opening = api.openPreview([fileEntry("first.txt"), fileEntry("last.txt")]);
  await statStarted.promise;
  assert.equal(api.openFile.value.path, "/home/last.txt");

  assert.equal(await api.closeEditor(), true);
  statPending.resolve({ size: 5, modified: 1700000000, mime: "text/plain" });
  await opening;
  assert.equal(api.openFile.value, null);
  assert.equal(mockSftpState.statCalls, 1);
  assert.equal(mockSftpState.readCalls, 0);
  assert.equal(serviceCalls("readRemoteSftpFileBytes").length, 0);
});

test("预览以新鲜 stat 覆盖列表中的旧大小和编辑能力", async () => {
  mockSftpState.stat = { size: 5, modified: 1700000000, mime: "text/plain" };
  const api = createApi();
  await api.openPreview([{ ...fileEntry("a.txt"), size: PREVIEW_BINARY_MAX_BYTES + 1 }]);

  assert.equal(api.openFile.value.tooLarge, false);
  assert.equal(api.openFile.value.editable, true);
  assert.ok(api.openFile.value.blob instanceof Blob);
  assert.equal(mockSftpState.readCalls, 1);
});

test("新鲜 stat 超过编辑上限时忽略列表里的旧 editable", async () => {
  mockSftpState.stat = {
    size: PREVIEW_TEXT_MAX_BYTES + 1,
    modified: 1700000000,
    mime: "text/plain",
  };
  const api = createApi();
  await api.openPreview([fileEntry("a.txt")]);

  assert.equal(api.openFile.value.tooLarge, false);
  assert.equal(api.openFile.value.editable, false);
  assert.equal(mockSftpState.readCalls, 0);
  assert.equal(serviceCalls("readRemoteSftpFileBytes").length, 1);
});

test("弱 text/plain 提示让未知扩展名可预览但不覆盖 file.mime", async () => {
  mockSftpState.stat = { size: 5, modified: 1700000000, mime: "text/plain" };
  const api = createApi();
  await api.openPreview([fileEntry("data.custom")]);

  assert.equal(api.openFile.value.mime, "");
  assert.equal(api.openFile.value.blob.type, "text/plain");
  assert.equal(serviceCalls("readRemoteSftpFileBytes").length, 1);
});

test("弱 text/plain 提示不覆盖扩展名 mime", async () => {
  mockSftpState.stat = { size: 5, modified: 1700000000, mime: "text/plain" };
  const api = createApi();
  await api.openPreview([fileEntry("photo.png")]);

  assert.equal(api.openFile.value.mime, "image/png");
  assert.equal(api.openFile.value.blob.type, "image/png");
});

test("同路径关闭重开后旧 convert 不得污染新实例", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("same.txt")]);
  const originalId = api.openFile.value.id;
  const oldRead = createDeferred();
  const oldReadStarted = createDeferred();
  let firstRead = true;
  mockSftpState.handlers.readRemoteSftpFile = () => {
    if (firstRead) {
      firstRead = false;
      oldReadStarted.resolve();
      return oldRead.promise;
    }
    return "new preview";
  };

  const converting = api.convertToEdit();
  await oldReadStarted.promise;
  assert.equal(await api.closeEditor(), true);
  await api.openPreview([fileEntry("same.txt")]);
  const reopened = api.openFile.value;
  assert.notEqual(reopened.id, originalId);
  assert.equal(reopened.mode, "preview");

  oldRead.resolve("stale edit");
  assert.equal(await converting, false);
  assert.equal(api.openFile.value, reopened);
  assert.equal(api.openFile.value.mode, "preview");
  assert.equal(api.openFile.value.contentLoaded, false);
  assert.equal(await api.openFile.value.blob.text(), "new preview");
});

test("同路径关闭重开后旧 save 不得污染新实例", async () => {
  const api = createApi({ requestDirtyAction: async () => "discard" });
  await api.openEditor(fileEntry("same.txt"));
  const originalId = api.openFile.value.id;
  api.updateEditorContent("old content");
  const oldWrite = createDeferred();
  const oldWriteStarted = createDeferred();
  mockSftpState.handlers.writeRemoteSftpFile = () => {
    oldWriteStarted.resolve();
    return oldWrite.promise;
  };

  const saving = api.saveEditor();
  await oldWriteStarted.promise;
  assert.equal(await api.closeEditor(), true);
  await api.openEditor(fileEntry("same.txt"));
  const reopened = api.openFile.value;
  assert.notEqual(reopened.id, originalId);
  const reopenedModified = reopened.modified;

  oldWrite.resolve({ modified: 1700000999 });
  assert.equal(await saving, false);
  assert.equal(api.openFile.value, reopened);
  assert.equal(api.openFile.value.content, "hello text");
  assert.equal(api.openFile.value.savedContent, "hello text");
  assert.equal(api.openFile.value.modified, reopenedModified);
});

test("write 和 refresh 期间的新输入不会进入 savedContent 快照", async () => {
  const writePending = createDeferred();
  const writeStarted = createDeferred();
  const refreshPending = createDeferred();
  const refreshStarted = createDeferred();
  mockSftpState.handlers.writeRemoteSftpFile = () => {
    writeStarted.resolve();
    return writePending.promise;
  };
  const api = createApi({
    refreshCurrentDirectoryIncremental: () => {
      refreshStarted.resolve();
      return refreshPending.promise;
    },
  });
  await api.openEditor(fileEntry("b.txt"));
  api.updateEditorContent("snapshot");

  const saving = api.saveEditor();
  await writeStarted.promise;
  api.updateEditorContent("typed during write");
  writePending.resolve({ modified: 1700000001 });
  await refreshStarted.promise;
  api.updateEditorContent("typed during refresh");
  refreshPending.resolve();

  assert.equal(await saving, true);
  const file = api.openFile.value;
  assert.equal(serviceCalls("writeRemoteSftpFile")[0].args[3], "snapshot");
  assert.equal(file.savedContent, "snapshot");
  assert.equal(file.content, "typed during refresh");
  assert.notEqual(file.content, file.savedContent);
  assert.equal(file.contentLoaded, true);
  assert.equal(file.saving, false);
});

for (const [currentModified, latestModified] of [
  [0, 1],
  [1, 0],
]) {
  test(`modified=${currentModified}、latest=${latestModified} 时仍执行覆盖检测`, async () => {
    mockSftpState.stat = { size: 5, modified: currentModified, mime: "text/plain" };
    let overwriteRequests = 0;
    const api = createApi({
      requestOverwriteAction: async () => {
        overwriteRequests += 1;
        return false;
      },
    });
    await api.openEditor(fileEntry("b.txt"));
    api.updateEditorContent("changed content");
    mockSftpState.stat = { size: 5, modified: latestModified, mime: "text/plain" };

    assert.equal(await api.saveEditor(), false);
    assert.equal(overwriteRequests, 1);
    assert.equal(serviceCalls("writeRemoteSftpFile").length, 0);
    assert.equal(api.openFile.value.saving, false);
    assert.equal(api.openFile.value.contentLoaded, true);
  });
}

test("convert 失败保留预览 blob 且可重试", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("a.txt")]);
  const blob = api.openFile.value.blob;
  let attempts = 0;
  mockSftpState.handlers.readRemoteSftpFile = async () => {
    attempts += 1;
    if (attempts === 1) throw new Error("convert denied");
    return "retried content";
  };

  assert.equal(await api.convertToEdit(), false);
  const failed = api.openFile.value;
  assert.equal(failed.mode, "preview");
  assert.equal(failed.blob, blob);
  assert.equal(failed.contentLoaded, false);
  assert.equal(failed.loading, false);
  assert.match(failed.error, /convert denied/);

  assert.equal(await api.convertToEdit(), true);
  const converted = api.openFile.value;
  assert.equal(converted.mode, "edit");
  assert.equal(converted.content, "retried content");
  assert.equal(converted.savedContent, "retried content");
  assert.equal(converted.contentLoaded, true);
  assert.equal(converted.blob, null);
  assert.equal(converted.error, "");
});

test("OpenFileView 将 mtime=0 格式化为 epoch 时间", async () => {
  const view = await setupSfc(
    new URL("../src/components/sftp/SftpOpenFileView.vue", import.meta.url),
    openFileViewProps(),
  );

  assert.equal(view.formatMtime({ mtime: 0 }), new Date(0).toLocaleString());
  assert.equal(view.formatMtime({ mtime: null }), "-");
});

test("同名邮件附件优先保存资源且 fetch/保存错误进入 OpenFileView", async () => {
  const view = await setupSfc(
    new URL("../src/components/sftp/SftpOpenFileView.vue", import.meta.url),
    openFileViewProps(),
  );
  let mainDownloads = 0;
  const viewer = await setupSfc(
    new URL("../src/components/sftp/SftpOpenFileViewer.vue", import.meta.url),
    {
      file: new Blob(["mail"]),
      fileName: "mail.eml",
      onDownload: () => {
        mainDownloads += 1;
      },
      onDownloadResource: view.saveResource,
      onResourceError: view.showResourceError,
    },
  );
  const originalFetch = globalThis.fetch;
  let fetches = 0;
  const successfulFetch = async () => {
    fetches += 1;
    return { arrayBuffer: async () => Uint8Array.from([65, 66]).buffer };
  };

  try {
    globalThis.fetch = successfulFetch;
    await viewer.routeDownload(downloadAnchor("mail.eml", false));
    assert.equal(mainDownloads, 1);
    assert.equal(fetches, 0);
    assert.equal(serviceCalls("saveSftpPreviewResource").length, 0);

    await viewer.routeDownload(downloadAnchor("mail.eml"));
    assert.equal(mainDownloads, 1);
    assert.equal(fetches, 1);
    const saveCalls = serviceCalls("saveSftpPreviewResource");
    assert.equal(saveCalls.length, 1);
    assert.deepEqual(saveCalls[0].args[0], {
      defaultFileName: "mail.eml",
      contentBase64: "QUI=",
      title: "Save resource",
    });

    mockSftpState.handlers.saveSftpPreviewResource = async () => {
      throw new Error("resource save denied");
    };
    await viewer.routeDownload(downloadAnchor("mail.eml"));
    assert.match(view.resourceError.value, /Resource save failed: resource save denied/);

    globalThis.fetch = async () => {
      throw new Error("resource fetch denied");
    };
    await viewer.routeDownload(downloadAnchor("mail.eml"));
    assert.match(view.resourceError.value, /Resource save failed: resource fetch denied/);

    delete mockSftpState.handlers.saveSftpPreviewResource;
    globalThis.fetch = successfulFetch;
    await viewer.routeDownload(downloadAnchor("mail.eml"));
    assert.equal(view.resourceError.value, "");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("同一文件打开失败后再次打开会重新读取", async () => {
  let attempts = 0;
  mockSftpState.handlers.readRemoteSftpFile = async () => {
    attempts += 1;
    if (attempts === 1) throw new Error("temporary read failure");
    return "recovered content";
  };
  const api = createApi();
  await api.openEditor(fileEntry("retry.txt"));
  const failedId = api.openFile.value.id;

  await api.openEditor(fileEntry("retry.txt"));

  assert.equal(attempts, 2);
  assert.notEqual(api.openFile.value.id, failedId);
  assert.equal(api.openFile.value.contentLoaded, true);
  assert.equal(api.openFile.value.content, "recovered content");
});

test("SFTP 能力失效后丢弃打开文件", async () => {
  const reactiveProps = reactive({
    connection: { id: "c1", capabilities: { sftp: true } },
    sessionId: "s1",
  });
  const api = useSftpOpenFiles({ props: reactiveProps, t });
  await api.openEditor(fileEntry("b.txt"));
  assert.ok(api.openFile.value);

  reactiveProps.connection = {
    ...reactiveProps.connection,
    capabilities: { sftp: false },
  };
  await nextTick();

  assert.equal(api.openFile.value, null);
});
