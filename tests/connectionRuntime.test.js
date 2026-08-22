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
