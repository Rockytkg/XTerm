import assert from "node:assert/strict";
import test from "node:test";
import {
  deserializeScriptError,
  SCRIPT_ERROR_CODES,
  ScriptCommandError,
  ScriptStoppedError,
  ScriptWaitTimeoutError,
  serializeScriptError,
} from "../src/services/scripting/scriptErrors.js";

test("script errors expose stable codes", () => {
  assert.equal(new ScriptStoppedError().code, SCRIPT_ERROR_CODES.STOPPED);
  assert.equal(new ScriptWaitTimeoutError("timeout").code, SCRIPT_ERROR_CODES.WAIT_TIMEOUT);
  assert.equal(new ScriptCommandError("failed", "dir").code, SCRIPT_ERROR_CODES.COMMAND);
});

test("script error serialization preserves name, message and code", () => {
  const source = new ScriptWaitTimeoutError("timed out", { pattern: "#", timeout: 50 });
  const restored = deserializeScriptError(serializeScriptError(source));
  assert.equal(restored.name, "ScriptWaitTimeoutError");
  assert.equal(restored.message, "timed out");
  assert.equal(restored.code, SCRIPT_ERROR_CODES.WAIT_TIMEOUT);
});

test("script error serialization tolerates non-Error values", () => {
  assert.deepEqual(serializeScriptError("plain failure"), {
    name: "Error",
    message: "plain failure",
  });
  const restored = deserializeScriptError({ message: "rpc failure" });
  assert.equal(restored.name, "Error");
  assert.equal(restored.message, "rpc failure");
  assert.equal(restored.code, undefined);
});
