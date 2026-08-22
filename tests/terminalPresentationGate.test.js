import assert from "node:assert/strict";
import test from "node:test";
import { createTerminalPresentationGate } from "../src/stores/terminalPresentationGate.js";

test("connection waits until its terminal status has been presented", async () => {
  const gate = createTerminalPresentationGate({ timeoutMs: 100 });
  const pending = gate.wait("session-1");

  assert.equal(gate.ready("session-1"), true);
  assert.equal(await pending, "ready");
});

test("connection presentation wait has a fallback when no terminal can mount", async () => {
  const gate = createTerminalPresentationGate({ timeoutMs: 5 });

  assert.equal(await gate.wait("session-1"), "timeout");
});

test("cancelled presentation wait cannot release a connection open", async () => {
  const gate = createTerminalPresentationGate({ timeoutMs: 100 });
  const pending = gate.wait("session-1");

  assert.equal(gate.cancel("session-1"), true);
  assert.equal(await pending, "cancelled");
});
