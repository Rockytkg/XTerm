import assert from "node:assert/strict";
import test from "node:test";
import { CONNECTION_EVENT } from "../src/stores/connectionStateMachine.js";
import { applyOpenResponseMetadata } from "../src/stores/workspaceRuntimeSync.js";
import { createWorkspaceSessionRegistry } from "../src/stores/workspaceSessionRegistry.js";

test("an early backend failure is buffered until the stable frontend session is bound", () => {
  const registry = createWorkspaceSessionRegistry();
  const frontendSessionId = "terminal-frontend";
  const backendSessionId = "telnet-backend";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  assert.deepEqual(
    registry.dispatchBackendConnectionEvent(backendSessionId, "connection-1", {
      type: CONNECTION_EVENT.SESSION_FAILED,
      payload: { detail: "Telnet negotiation failed" },
    }),
    { routing: "buffered" },
  );
  assert.equal(registry.bindBackendSession(frontendSessionId, backendSessionId, 1), true);

  applyOpenResponseMetadata({
    dispatchConnectionEvent: registry.dispatchConnectionEvent,
    response: { status: "connected", sessionId: backendSessionId, capabilities: {} },
    sessionId: frontendSessionId,
    sessionRegistry: registry,
  });

  assert.equal(registry.getBackendSessionId(frontendSessionId), "");
  assert.equal(registry.getConnectionState(frontendSessionId).status, "failed");
});

test("beginning a new attempt resets runtime metadata and retires the old backend session", () => {
  const registry = createWorkspaceSessionRegistry();
  const frontendSessionId = "session-1";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.bindBackendSession(frontendSessionId, "backend-1", 1);
  registry.setRuntimeMetrics(frontendSessionId, { latencyMs: 12 });

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 2, "open-2");

  assert.equal(registry.getRuntimeMetrics(frontendSessionId), null);
  assert.equal(registry.getBackendSessionId(frontendSessionId), "");
  assert.deepEqual(
    registry.dispatchBackendConnectionEvent("backend-1", "connection-1", {
      type: CONNECTION_EVENT.SESSION_FAILED,
      payload: { detail: "late failure" },
    }),
    { routing: "stale" },
  );
  assert.equal(registry.getConnectionState(frontendSessionId).status, "connecting");
});

test("a backend session cannot bind to a superseded connection attempt", () => {
  const registry = createWorkspaceSessionRegistry();
  const frontendSessionId = "telnet-session";

  registry.beginSessionAttempt(frontendSessionId, "connection-1", 1, "open-1");
  registry.beginSessionAttempt(frontendSessionId, "connection-1", 2, "open-2");

  assert.equal(registry.getFrontendSessionIdForOpenRequest("open-1"), "");
  assert.equal(registry.getFrontendSessionIdForOpenRequest("open-2"), frontendSessionId);
  assert.equal(registry.bindBackendSession(frontendSessionId, "backend-old", 1), false);
  assert.equal(registry.bindBackendSession(frontendSessionId, "backend-current", 2), true);
  assert.equal(registry.getFrontendSessionId("backend-current"), frontendSessionId);
});

test("SSH and Telnet connections can bind multiple independent backend sessions", () => {
  for (const protocol of ["ssh", "telnet"]) {
    const registry = createWorkspaceSessionRegistry();
    const connectionId = `${protocol}-connection`;
    const firstTab = `${protocol}-tab-1`;
    const secondTab = `${protocol}-tab-2`;
    const firstBackend = `${protocol}-backend-1`;
    const secondBackend = `${protocol}-backend-2`;

    registry.beginSessionAttempt(firstTab, connectionId, 1, `${protocol}-open-1`);
    registry.beginSessionAttempt(secondTab, connectionId, 2, `${protocol}-open-2`);
    assert.equal(registry.bindBackendSession(firstTab, firstBackend, 1), true);
    assert.equal(registry.bindBackendSession(secondTab, secondBackend, 2), true);

    assert.equal(registry.getBackendSessionId(firstTab), firstBackend);
    assert.equal(registry.getBackendSessionId(secondTab), secondBackend);
    assert.equal(registry.getFrontendSessionId(firstBackend), firstTab);
    assert.equal(registry.getFrontendSessionId(secondBackend), secondTab);
  }
});
