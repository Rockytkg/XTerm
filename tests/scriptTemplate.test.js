import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_SCRIPT_BODY } from "../src/services/scripting/scriptTemplate.js";

test("default script template is comment-only and documents terminal APIs", () => {
  const meaningfulLines = DEFAULT_SCRIPT_BODY.split(/\r?\n/).filter((line) => line.trim());
  assert.ok(meaningfulLines.length > 0);
  assert.ok(meaningfulLines.every((line) => line.trimStart().startsWith("//")));
  assert.doesNotMatch(DEFAULT_SCRIPT_BODY, /\bcrt\b/i);

  for (const api of [
    "xterm.send(",
    "xterm.sendLine(",
    "xterm.waitFor(",
    "xterm.waitForAny(",
    "xterm.read(",
    "xterm.getScreen(",
    "xterm.sleep(",
    "xterm.input(",
    "xterm.confirm(",
    "xterm.alert(",
    "xterm.form(",
    "xterm.log(",
    "xterm.session.id",
    "xterm.session.label",
  ]) {
    assert.match(DEFAULT_SCRIPT_BODY, new RegExp(api.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
});
