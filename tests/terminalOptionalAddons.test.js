import assert from "node:assert/strict";
import test, { afterEach, beforeEach, mock } from "node:test";
import { useTerminalOptionalAddons } from "../src/composables/useTerminalOptionalAddons.js";

// 动态 import（@xterm/addon-webgl）与异步恢复链都需要真实的宏任务推进，
// 而被 mock 的只有定时器，setImmediate 仍可用来冲刷微任务与动态导入。
async function flushAsyncWork() {
  for (let index = 0; index < 5; index += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function createHarness({ detectRenderer, loadAddonImpl } = {}) {
  const logs = [];
  const terminal = {
    rows: 24,
    loadAddon: mock.fn((addon) => loadAddonImpl?.(addon)),
    refresh: mock.fn(),
    dispose: () => {},
  };
  const context = {
    generation: 1,
    disposed: false,
    terminal,
    terminalWebgl: true,
    terminalCustomGlyphs: false,
    isForegroundRuntime: true,
  };
  const addons = useTerminalOptionalAddons({
    logger: {
      info: (...args) => logs.push(["info", ...args]),
      warn: (...args) => logs.push(["warn", ...args]),
      error: () => {},
      debug: () => {},
    },
    getContext: () => context,
    loadAddon: () => {},
    detectRenderer:
      detectRenderer ??
      (async () => ({ renderer: "ANGLE (NVIDIA GeForce RTX 4070)", software: false })),
  });
  return { addons, context, logs, terminal };
}

function loadedAddon(terminal, index) {
  return terminal.loadAddon.mock.calls[index].arguments[0];
}

beforeEach(() => {
  mock.timers.enable({ apis: ["setTimeout"] });
});

afterEach(() => {
  mock.timers.reset();
});

test("transient recovery failures keep retrying instead of latching the generation", async () => {
  let failRecovery = false;
  const { addons, terminal } = createHarness({
    loadAddonImpl: () => {
      if (failRecovery) throw new Error("WebGL2 not supported");
    },
  });

  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 1);

  // GPU 重置：上下文丢失，前两次恢复尝试仍失败（getContext 返回 null）。
  failRecovery = true;
  loadedAddon(terminal, 0)._onContextLoss.fire();
  mock.timers.tick(2_000);
  await flushAsyncWork();
  assert.equal(terminal.loadAddon.mock.callCount(), 2);

  // 关键：第二次退避必须真正重试（旧实现里闩锁会让第 2/3 次尝试空转）。
  mock.timers.tick(5_000);
  await flushAsyncWork();
  assert.equal(terminal.loadAddon.mock.callCount(), 3);

  // 第三次退避恢复成功，之后常规 sync 不应重复加载。
  failRecovery = false;
  mock.timers.tick(15_000);
  await flushAsyncWork();
  assert.equal(terminal.loadAddon.mock.callCount(), 4);
  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 4);
});

test("software rasterizer keeps the DOM renderer and latches the generation", async () => {
  const { addons, logs, terminal } = createHarness({
    detectRenderer: async () => ({
      renderer: "ANGLE (Microsoft, Microsoft Basic Render Driver Direct3D11 vs_5_0 ps_5_0, D3D11)",
      software: true,
    }),
  });

  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 0);
  assert.ok(
    logs.some(
      ([level, message]) => level === "warn" && /software rasterized/.test(String(message)),
    ),
  );

  // 同一 generation 内不再反复探测，等开关切换/终端重建复位。
  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 0);
});

test("reloading the WebGL renderer applies addon constructor options", async () => {
  const { addons, terminal } = createHarness();

  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 1);

  await addons.reloadWebglRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 2);
});

test("rapid context loss loops stop after the stability window is never reached", async () => {
  const { addons, terminal } = createHarness();

  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 1);

  for (let loss = 0; loss < 3; loss += 1) {
    loadedAddon(terminal, loss)._onContextLoss.fire();
    if (loss < 2) {
      mock.timers.tick(2_000);
      await flushAsyncWork();
    }
  }
  assert.equal(terminal.loadAddon.mock.callCount(), 3);

  // 连续 3 次快速丢失后停止恢复：不再安排退避，也不再加载。
  mock.timers.tick(15_000);
  await flushAsyncWork();
  assert.equal(terminal.loadAddon.mock.callCount(), 3);

  // 闩锁期间常规 sync 也不会重试。
  await addons.syncTerminalRenderer();
  assert.equal(terminal.loadAddon.mock.callCount(), 3);
});

test("renderer swaps do not schedule duplicate application-level refreshes", async () => {
  const { addons, terminal } = createHarness();

  await addons.syncTerminalRenderer();
  assert.equal(terminal.refresh.mock.callCount(), 0);

  loadedAddon(terminal, 0)._onContextLoss.fire();
  assert.equal(terminal.refresh.mock.callCount(), 0);
});
