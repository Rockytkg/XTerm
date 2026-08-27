import assert from "node:assert/strict";
import test from "node:test";
import { createTerminalSessionRuntimeController } from "../src/utils/terminal/TerminalSessionRuntimeController.js";

test("initial connection status survives a queued deactivate without a channel", async () => {
  let dropCount = 0;
  const statuses = [];
  const controller = createTerminalSessionRuntimeController({
    drainOutput: () => Promise.resolve(),
    dropOutput: () => {
      dropCount += 1;
    },
    getContext: () => ({
      connectionId: "connection-1",
      connectionStatus: "connecting",
      disposed: false,
      hasActiveConnection: true,
      isForeground: true,
      sessionId: "",
      terminalReady: true,
    }),
    logger: null,
    queueResizeSync: () => {},
    setActiveSessionChannel: () => {},
    transport: { send: () => Promise.resolve() },
    writeStatus: (status) => statuses.push(status),
  });

  controller.syncRuntimeResources();
  controller.handleConnectionStatus("connecting", null);
  await Promise.resolve();

  assert.deepEqual(statuses, ["connecting"]);
  assert.equal(dropCount, 0);
});

test("connected lifecycle status is rendered from workspace state", () => {
  const statuses = [];
  const controller = createTerminalSessionRuntimeController({
    drainOutput: () => Promise.resolve(),
    dropOutput: () => {},
    getContext: () => ({
      connectionId: "connection-1",
      connectionStatus: "connected",
      disposed: false,
      hasActiveConnection: true,
      isForeground: true,
      sessionId: "",
      terminalReady: true,
    }),
    logger: null,
    queueResizeSync: () => {},
    setActiveSessionChannel: () => {},
    transport: { send: () => Promise.resolve() },
    writeStatus: (status) => statuses.push(status),
  });

  controller.handleConnectionStatus("connected", null);

  assert.deepEqual(statuses, ["connected"]);
});

test("non-presentational disconnect states release stale connection progress", () => {
  const released = [];
  const controller = createTerminalSessionRuntimeController({
    drainOutput: () => Promise.resolve(),
    dropOutput: () => {},
    getContext: () => ({
      connectionId: "connection-1",
      disposed: false,
      hasActiveConnection: true,
      isForeground: true,
      sessionId: "",
      terminalReady: true,
    }),
    logger: null,
    queueResizeSync: () => {},
    releaseStatus: () => released.push("release"),
    setActiveSessionChannel: () => {},
    transport: { send: () => Promise.resolve() },
    writeStatus: () => {},
  });

  controller.handleConnectionStatus("disconnecting", "disconnecting");
  controller.handleConnectionStatus("idle", null);

  assert.deepEqual(released, ["release", "release"]);
});

test("terminal input uses one monotonic sequence per active channel", async () => {
  const frames = [];
  const context = {
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
        frames.push(frame);
        return Promise.resolve();
      },
    },
    writeStatus: () => {},
  });
  controller.channel = { sessionId: "session-1", channelId: 7 };

  controller.queueText("\r");
  controller.queueText("show version\r");
  await Promise.resolve();

  assert.deepEqual(
    frames.map(({ inputSequence, data }) => ({ inputSequence, data })),
    [
      { inputSequence: 1, data: "\r" },
      { inputSequence: 2, data: "show version\r" },
    ],
  );
});

test("terminal input is discarded while serial baud detection owns the port", () => {
  const frames = [];
  const controller = createTerminalSessionRuntimeController({
    drainOutput: () => Promise.resolve(),
    dropOutput: () => {},
    getContext: () => ({
      connectionId: "connection-1",
      connectionPhase: "serialBaudDetection",
      connectionStatus: "connected",
      disposed: false,
      hasActiveConnection: true,
      isForeground: true,
      sessionId: "session-1",
      terminalReady: true,
    }),
    logger: null,
    queueResizeSync: () => {},
    setActiveSessionChannel: () => {},
    transport: { send: (frame) => frames.push(frame) },
    writeStatus: () => {},
  });
  controller.channel = { sessionId: "session-1", channelId: 7 };

  controller.queueText("\r");

  assert.deepEqual(frames, []);
});
