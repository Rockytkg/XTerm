import assert from "node:assert/strict";
import test from "node:test";
import { createConnectionRuntime } from "../src/stores/connectionRuntime.js";

test("a new connection attempt invalidates promises from the previous attempt", () => {
  const runtime = createConnectionRuntime();
  const firstAttempt = runtime.begin("session-1");
  const secondAttempt = runtime.begin("session-1");

  assert.equal(runtime.isCurrent("session-1", firstAttempt), false);
  assert.equal(runtime.isCurrent("session-1", secondAttempt), true);

  runtime.finish("session-1", firstAttempt);
  assert.equal(runtime.isCurrent("session-1", secondAttempt), true);

  runtime.finish("session-1", secondAttempt);
  assert.equal(runtime.isPending("session-1"), false);
});

test("finish releases the runtime so a retry can begin (host-key prompt cancel path)", () => {
  const runtime = createConnectionRuntime();
  const firstAttempt = runtime.begin("session-1");

  runtime.finish("session-1", firstAttempt);

  const retryAttempt = runtime.begin("session-1");
  assert.notEqual(retryAttempt, null);
  assert.equal(runtime.isCurrent("session-1", retryAttempt), true);
});

test("cancel blocks begin until closeComplete (session close in flight)", () => {
  const runtime = createConnectionRuntime();
  const attempt = runtime.begin("session-1");

  runtime.cancel("session-1", attempt);
  assert.equal(runtime.begin("session-1"), null);

  runtime.closeComplete("session-1");
  assert.notEqual(runtime.begin("session-1"), null);
});
