import assert from "node:assert/strict";
import test from "node:test";
import { focusTerminal, registerScriptBridge } from "../src/services/scripting/bridges.js";

test("focusTerminal delegates focus to the registered terminal bridge", () => {
  let focusCalls = 0;
  const bridge = {
    focus() {
      focusCalls += 1;
    },
  };
  const unregister = registerScriptBridge("focus-test-session", bridge);

  try {
    assert.equal(focusTerminal("focus-test-session"), true);
    assert.equal(focusCalls, 1);
  } finally {
    unregister();
  }
});

test("focusTerminal is best-effort for missing or failing bridges", () => {
  assert.equal(focusTerminal("missing-focus-test-session"), false);

  const unregister = registerScriptBridge("failing-focus-test-session", {
    focus() {
      throw new Error("disposed terminal");
    },
  });

  try {
    assert.equal(focusTerminal("failing-focus-test-session"), false);
  } finally {
    unregister();
  }
});
