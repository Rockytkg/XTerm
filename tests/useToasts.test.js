import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { showToast, useToasts } from "../src/composables/useToasts.js";

// node --test 环境没有 window；useToasts 通过 window.setTimeout 调度，
// 这里用委托 stub 保证拿到的是 mock.timers 替换后的全局函数。
globalThis.window = {
  setTimeout: (...args) => setTimeout(...args),
  clearTimeout: (id) => clearTimeout(id),
};
mock.timers.enable({ apis: ["setTimeout"] });

function findToast(id) {
  return useToasts().toasts.value.find((toast) => toast.id === id);
}

test("showToast defaults duration by type", () => {
  const infoId = showToast({ type: "info", title: "i" });
  assert.equal(findToast(infoId).duration, 3_200);

  const loadingId = showToast({ type: "loading", title: "l" });
  assert.equal(findToast(loadingId).duration, 600_000);
});

test("updateToast drops the loading duration so result toasts auto-dismiss", () => {
  // 回归：loading（10 分钟兜底）更新为 success 后曾沿用旧时长，提示迟迟不消失。
  const id = showToast({ type: "loading", title: "detecting" });
  assert.equal(findToast(id).duration, 600_000);

  useToasts().updateToast(id, { type: "success", title: "done" });
  assert.equal(findToast(id).duration, 3_200);

  mock.timers.tick(3_200);
  assert.equal(findToast(id).open, false);
  mock.timers.tick(180); // 退场动画结束后移除
  assert.equal(findToast(id), undefined);
});

test("updateToast respects an explicit duration in the patch", () => {
  const id = showToast({ type: "loading", title: "x" });
  useToasts().updateToast(id, { type: "error", title: "y", duration: 5_000 });
  assert.equal(findToast(id).duration, 5_000);

  mock.timers.tick(5_000);
  assert.equal(findToast(id).open, false);
});
