import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { DIALOG_EXIT_MS, useDialogExitTeardown } from "../src/composables/useDialogExitTeardown.js";

// node --test 环境没有 window；composable 通过 window.setTimeout 调度，
// 这里用委托 stub 保证拿到的是 mock.timers 替换后的全局函数。
globalThis.window = {
  setTimeout: (...args) => setTimeout(...args),
  clearTimeout: (id) => clearTimeout(id),
};
mock.timers.enable({ apis: ["setTimeout"] });

test("exit teardown runs after the dialog exit animation window", () => {
  const { scheduleExitTeardown } = useDialogExitTeardown();
  let calls = 0;
  scheduleExitTeardown(() => {
    calls += 1;
  });

  mock.timers.tick(DIALOG_EXIT_MS - 1);
  assert.equal(calls, 0, "teardown must not run before the exit animation ends");
  mock.timers.tick(1);
  assert.equal(calls, 1);
});

test("reopening before the deadline cancels the pending teardown", () => {
  const { scheduleExitTeardown, cancelExitTeardown } = useDialogExitTeardown();
  let calls = 0;
  scheduleExitTeardown(() => {
    calls += 1;
  });
  cancelExitTeardown();

  mock.timers.tick(DIALOG_EXIT_MS + 100);
  assert.equal(calls, 0);
});

test("scheduling again replaces the previous teardown", () => {
  const { scheduleExitTeardown } = useDialogExitTeardown();
  const order = [];
  scheduleExitTeardown(() => order.push("stale"));
  scheduleExitTeardown(() => order.push("fresh"));

  mock.timers.tick(DIALOG_EXIT_MS + 100);
  assert.deepEqual(order, ["fresh"]);
});
