import assert from "node:assert/strict";
import test from "node:test";
import { ref } from "vue";
import {
  createSftpDragNavigator,
  isSameOrChildPath,
  normalizeRemotePath,
} from "../src/composables/sftpDragNavigation.js";

let windowStub;

function createWindowStub() {
  let nextTimerId = 1;
  const timers = new Map();
  return {
    timers,
    setTimeout(callback) {
      const id = nextTimerId++;
      timers.set(id, callback);
      return id;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
  };
}

function pointAtDirectory(path) {
  const browser = { contains: () => true };
  const row = path
    ? {
        dataset: { path },
        classList: { contains: () => false },
      }
    : null;
  const element = {
    closest(selector) {
      if (selector === ".sftp-browser") return browser;
      if (selector === ".sftp-row.is-dir[data-path]") return row;
      return null;
    },
  };
  globalThis.document = { elementFromPoint: () => element };
}

async function runNextTimer() {
  const next = windowStub.timers.entries().next();
  assert.equal(next.done, false, "expected a pending navigation timer");
  const [id, callback] = next.value;
  windowStub.timers.delete(id);
  callback();
  await new Promise((resolve) => setImmediate(resolve));
}

test.beforeEach(() => {
  windowStub = createWindowStub();
  globalThis.window = windowStub;
});

test("normalizeRemotePath 处理空路径、点段、重复分隔符和相对上级", () => {
  const cases = [
    ["", "."],
    [null, "."],
    ["/", "/"],
    ["/a//./b/../c/", "/a/c"],
    ["/../../a", "/a"],
    ["a//b/", "a/b"],
    ["a/../../b", "../b"],
    ["../a/./b/..", "../a"],
  ];
  for (const [input, expected] of cases) {
    assert.equal(normalizeRemotePath(input), expected, input);
  }
});

test("isSameOrChildPath 先规范化且按完整路径段判断父子关系", () => {
  assert.equal(isSameOrChildPath("/a/b", "/a/./c/../b"), true);
  assert.equal(isSameOrChildPath("/a/b/c", "/a/b"), true);
  assert.equal(isSameOrChildPath("/a/bc", "/a/b"), false);
  assert.equal(isSameOrChildPath("/a", "/"), true);
  assert.equal(isSameOrChildPath("a/b", "a"), true);
  assert.equal(isSameOrChildPath("../a/b", "../a"), true);
  assert.equal(isSameOrChildPath("../ab", "../a"), false);
});

test("拖拽 A/B/A 返回当前目录时不在延迟计时器后重复刷新", async () => {
  const remotePath = ref("/a");
  const refreshCalls = [];
  const navigator = createSftpDragNavigator({
    remotePath,
    remoteParent: ref(""),
    refreshRemote: async (path) => {
      refreshCalls.push(path);
      remotePath.value = path;
      return true;
    },
    setDropTargetPath: () => {},
  });
  const session = { active: true, destinationDirectory: "/a" };

  pointAtDirectory("/b");
  navigator.updateFromPoint(0, 0, session);
  await runNextTimer();
  assert.equal(remotePath.value, "/b");
  assert.deepEqual(refreshCalls, ["/b"]);

  pointAtDirectory("/a");
  navigator.updateFromPoint(0, 0, session);
  assert.equal(windowStub.timers.size, 1);

  remotePath.value = "/a";
  await runNextTimer();
  assert.deepEqual(refreshCalls, ["/b"]);
});
