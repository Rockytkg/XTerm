import assert from "node:assert/strict";
import { register } from "node:module";
import test from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { mockSftpState, resetMockSftpState } = await import("./helpers/mockSftpService.mjs");
const { loadRemoteEntriesByName, NAME_CONFLICT_ACTION, remoteNameFromPath, resolveNameConflict } =
  await import("../src/composables/sftpRemoteOperations.js");

test.beforeEach(() => {
  resetMockSftpState();
});

test("remoteNameFromPath 保留根目录、相对路径与规范化后的名称", () => {
  assert.equal(remoteNameFromPath("/"), "/");
  assert.equal(remoteNameFromPath("."), ".");
  assert.equal(remoteNameFromPath("/home/user/"), "user");
  assert.equal(remoteNameFromPath("a/../b.txt"), "b.txt");
  assert.equal(remoteNameFromPath("../notes.txt"), "notes.txt");
  assert.equal(remoteNameFromPath("./~"), "~");
  assert.equal(remoteNameFromPath("./name "), "name ");
});

test("loadRemoteEntriesByName 缺会话时不发 IPC，返回名称索引", async () => {
  assert.equal((await loadRemoteEntriesByName({ path: "/" })).size, 0);
  assert.equal(mockSftpState.calls.length, 0);

  const entries = [
    { name: "a.txt", path: "/a.txt" },
    { name: "b.txt", path: "/b.txt" },
  ];
  mockSftpState.handlers.listRemoteSftp = async () => ({ entries });
  const result = await loadRemoteEntriesByName({
    connectionId: "c1",
    sessionId: "s1",
    path: "/",
  });
  assert.deepEqual([...result.keys()], ["a.txt", "b.txt"]);
  assert.equal(result.get("b.txt"), entries[1]);
});

test("resolveNameConflict 不询问自身或不存在目标", async () => {
  let prompts = 0;
  const requestConflictAction = async () => {
    prompts += 1;
    return NAME_CONFLICT_ACTION.OVERWRITE;
  };
  const source = { name: "a.txt", path: "/a.txt" };
  const targetFileByName = new Map([["a.txt", source]]);

  const same = await resolveNameConflict({
    sourcePath: source.path,
    targetFileByName,
    targetName: "a.txt",
    requestConflictAction,
  });
  const missing = await resolveNameConflict({
    targetFileByName,
    targetName: "b.txt",
    requestConflictAction,
    defaultAction: NAME_CONFLICT_ACTION.RESUME,
  });

  assert.equal(prompts, 0);
  assert.equal(same.action, NAME_CONFLICT_ACTION.CREATE);
  assert.equal(same.existing, null);
  assert.equal(missing.action, NAME_CONFLICT_ACTION.RESUME);
});

test("resolveNameConflict 保留 overwrite、cancel 与 skip 的调用契约", async () => {
  const existing = { name: "a.txt", path: "/target/a.txt" };
  const sourceEntry = { name: "a.txt", path: "/source/a.txt" };
  const targetFileByName = new Map([["a.txt", existing]]);

  for (const action of [
    NAME_CONFLICT_ACTION.OVERWRITE,
    NAME_CONFLICT_ACTION.CANCEL,
    NAME_CONFLICT_ACTION.SKIP,
  ]) {
    let payload;
    const result = await resolveNameConflict({
      sourceEntry,
      targetFileByName,
      targetName: "a.txt",
      requestConflictAction: async (request) => {
        payload = request;
        return action;
      },
    });
    assert.equal(result.action, action);
    assert.equal(result.existing, existing);
    assert.equal(result.cancelled, action === NAME_CONFLICT_ACTION.CANCEL);
    assert.equal(result.skipped, action === NAME_CONFLICT_ACTION.SKIP);
    assert.equal(payload.entry, existing);
    assert.equal(payload.sourceEntry, sourceEntry);
  }
});
