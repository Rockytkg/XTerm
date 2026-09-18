// useSftpOpenFiles 单文件打开模型测试。service 层经 loader 替换为内存桩（见 helpers/），
// 覆盖打开即加载完成、打开即替换的核心语义——也是 ref 深响应式代理导致
// 对象同一性守卫（openFile.value === tab）失效、加载结果全部被丢弃的回归防护。
import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { nextTick, reactive } = await import("vue");
const { mockSftpState, resetMockSftpState } = await import("./helpers/mockSftpService.mjs");
const { useSftpOpenFiles } = await import("../src/composables/useSftpOpenFiles.js");

const props = {
  connection: { id: "c1", capabilities: { sftp: true } },
  sessionId: "s1",
};
const t = (key) => key;

function createApi(overrides = {}) {
  return useSftpOpenFiles({ props, t, ...overrides });
}

function fileEntry(name) {
  return { name, path: `/home/${name}`, size: 5, kind: "file" };
}

test("openPreview 完成加载并生成预览 Blob", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("a.txt")]);
  const file = api.openFile.value;
  assert.equal(file.mode, "preview");
  assert.equal(file.loading, false);
  assert.equal(file.error, "");
  assert.ok(file.blob instanceof Blob);
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

test("convertToEdit 预览转编辑加载文本内容", async () => {
  const api = createApi();
  await api.openPreview([fileEntry("b.txt")]);
  assert.equal(api.openFile.value.mode, "preview");
  assert.equal(await api.convertToEdit(), true);
  const file = api.openFile.value;
  assert.equal(file.mode, "edit");
  assert.equal(file.loading, false);
  assert.equal(file.content, "hello text");
  assert.equal(file.savedContent, "hello text");
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
