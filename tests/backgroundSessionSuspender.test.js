import assert from "node:assert/strict";
import test from "node:test";
import { createBackgroundSessionSuspender } from "../src/utils/terminal/backgroundSessionSuspender.js";
import { createTerminalSessionRuntimeController } from "../src/utils/terminal/TerminalSessionRuntimeController.js";

function createFakeTimer() {
  const timers = new Map();
  let nextHandle = 1;
  return {
    setTimer(callback, ms) {
      const handle = nextHandle++;
      timers.set(handle, { callback, ms });
      return handle;
    },
    clearTimer(handle) {
      timers.delete(handle);
    },
    fireAll() {
      const pending = [...timers.values()];
      timers.clear();
      for (const { callback } of pending) callback();
    },
    get pendingCount() {
      return timers.size;
    },
  };
}

function createSuspenderHarness({ background = true } = {}) {
  const timer = createFakeTimer();
  const calls = [];
  let isBackground = background;
  const suspender = createBackgroundSessionSuspender({
    isBackground: () => isBackground,
    suspend: () => calls.push("suspend"),
    resume: () => calls.push("resume"),
    setTimer: timer.setTimer,
    clearTimer: timer.clearTimer,
  });
  return {
    suspender,
    timer,
    calls,
    setBackground(value) {
      isBackground = value;
    },
  };
}

test("suspend fires only after the delay once backgrounded", () => {
  const { suspender, timer, calls } = createSuspenderHarness();

  suspender.sync(false);
  assert.equal(timer.pendingCount, 1);
  assert.deepEqual(calls, []);

  timer.fireAll();
  assert.deepEqual(calls, ["suspend"]);
  assert.equal(suspender.suspended, true);
});

test("returning foreground before the delay cancels the suspend", () => {
  const { suspender, timer, calls } = createSuspenderHarness();

  suspender.sync(false);
  suspender.sync(true);
  assert.equal(timer.pendingCount, 0);

  timer.fireAll();
  assert.deepEqual(calls, []);
  assert.equal(suspender.suspended, false);
});

test("foreground after suspend triggers exactly one resume", () => {
  const { suspender, timer, calls } = createSuspenderHarness();

  suspender.sync(false);
  timer.fireAll();
  suspender.sync(true);
  suspender.sync(true);

  assert.deepEqual(calls, ["suspend", "resume"]);
});

test("rapid toggling does not stack timers or duplicate suspend", () => {
  const { suspender, timer, calls } = createSuspenderHarness();

  suspender.sync(false);
  suspender.sync(true);
  suspender.sync(false);
  suspender.sync(false);
  assert.equal(timer.pendingCount, 1);

  timer.fireAll();
  assert.deepEqual(calls, ["suspend"]);
});

test("timer firing after a silent foreground transition is a no-op", () => {
  const { suspender, timer, calls, setBackground } = createSuspenderHarness();

  suspender.sync(false);
  setBackground(false);
  timer.fireAll();

  assert.deepEqual(calls, []);
  assert.equal(suspender.suspended, false);
});

test("dispose cancels a pending suspend", () => {
  const { suspender, timer, calls } = createSuspenderHarness();

  suspender.sync(false);
  suspender.dispose();
  timer.fireAll();

  assert.deepEqual(calls, []);
});

function createControllerHarness() {
  const sentFrames = [];
  let context = {
    connectionId: "connection-1",
    connectionStatus: "connected",
    disposed: false,
    hasActiveConnection: true,
    isForeground: true,
    sessionId: "session-1",
    terminalReady: true,
  };
  const controller = createTerminalSessionRuntimeController({
    drainOutput: () => Promise.resolve(),
    dropOutput: () => {},
    getContext: () => context,
    logger: null,
    queueResizeSync: () => {},
    setActiveSessionChannel: () => {},
    transport: {
      send: (frame) => {
        sentFrames.push(frame);
        if (frame?.type === "terminal.attach") {
          return Promise.resolve({
            alreadyActive: false,
            channelId: 7,
            connectionId: "connection-1",
            sessionId: frame.sessionId,
            subscriptionId: 1,
          });
        }
        return Promise.resolve();
      },
    },
    writeStatus: () => {},
  });
  return {
    controller,
    sentFrames,
    setContext(patch) {
      context = { ...context, ...patch };
    },
  };
}

test("suspended controller refuses activation until resumed", async () => {
  const { controller, sentFrames } = createControllerHarness();

  await controller.activate();
  assert.equal(sentFrames.filter((frame) => frame.type === "terminal.attach").length, 1);
  assert.ok(controller.currentChannel());

  await controller.suspendForBackground();
  assert.equal(controller.currentChannel(), null);
  assert.equal(controller.isSuspended(), true);
  assert.equal(sentFrames.filter((frame) => frame.type === "terminal.detach").length, 1);

  // 挂起期间的常规激活入口（连接状态回放、syncRuntimeResources）不得重新 attach。
  await controller.activate();
  controller.syncRuntimeResources();
  await Promise.resolve();
  assert.equal(sentFrames.filter((frame) => frame.type === "terminal.attach").length, 1);

  await controller.resumeFromBackground();
  assert.equal(controller.isSuspended(), false);
  assert.equal(sentFrames.filter((frame) => frame.type === "terminal.attach").length, 2);
  assert.ok(controller.currentChannel());
});

test("resume re-attaches on the same session so replay can catch up incrementally", async () => {
  const { controller, sentFrames } = createControllerHarness();

  await controller.activate();
  await controller.suspendForBackground();
  await controller.resumeFromBackground();

  const attachFrames = sentFrames.filter((frame) => frame.type === "terminal.attach");
  assert.equal(attachFrames.length, 2);
  assert.equal(attachFrames[1].sessionId, "session-1");
});
