import assert from "node:assert/strict";
import test from "node:test";
import { CONNECTION_EVENT } from "../src/stores/connectionStateMachine.js";
import { createWorkspaceSessionRegistry } from "../src/stores/workspaceSessionRegistry.js";

test("retiring a bound backend session notifies the release callback (force-reconnect orphan)", () => {
  const released = [];
  const registry = createWorkspaceSessionRegistry({
    onRetireBackendSession: (backendSessionId) => released.push(backendSessionId),
  });
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);

  // force-reconnect：新 attempt 退休旧会话，旧后端会话应被通知回收。
  registry.beginSessionAttempt(frontendSessionId, "connection-1", 2, "open-2");

  assert.deepEqual(released, ["backend-1"]);
  // 退休集合语义不变：旧会话的迟到事件仍按 stale 拒绝（attemptToken 防串扰）。
  assert.deepEqual(
    registry.dispatchBackendConnectionEvent("backend-1", "connection-1", {
      type: CONNECTION_EVENT.SESSION_FAILED,
      payload: { detail: "late failure" },
    }),
    { routing: "stale" },
  );
});

test("replacing a bound backend session releases the previous one", () => {
  const released = [];
  const registry = createWorkspaceSessionRegistry({
    onRetireBackendSession: (backendSessionId) => released.push(backendSessionId),
  });
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);
  registry.bindBackendSession(frontendSessionId, "backend-2", 1);

  assert.deepEqual(released, ["backend-1"]);
  assert.equal(registry.getBackendSessionId(frontendSessionId), "backend-2");
});

test("backend-ended sessions are retired without a redundant release", () => {
  const released = [];
  const registry = createWorkspaceSessionRegistry({
    onRetireBackendSession: (backendSessionId) => released.push(backendSessionId),
  });
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);
  registry.dispatchBackendConnectionEvent("backend-1", "connection-1", {
    type: CONNECTION_EVENT.SESSION_CLOSED,
  });

  assert.deepEqual(released, []);
});

test("unbind can opt out of the release callback for explicit close paths", () => {
  const released = [];
  const registry = createWorkspaceSessionRegistry({
    onRetireBackendSession: (backendSessionId) => released.push(backendSessionId),
  });
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);
  const backendSessionId = registry.unbindBackendSession(frontendSessionId, {
    releaseBackend: false,
  });

  assert.equal(backendSessionId, "backend-1");
  assert.deepEqual(released, []);
});

test("without a callback the registry behaves as before", () => {
  const registry = createWorkspaceSessionRegistry();
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);

  assert.doesNotThrow(() =>
    registry.beginSessionAttempt(frontendSessionId, "connection-1", 2, "open-2"),
  );
  assert.equal(registry.getBackendSessionId(frontendSessionId), "");
});
