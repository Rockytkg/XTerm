import assert from "node:assert/strict";
import test from "node:test";
import {
  compileScript,
  formatScript,
  formatScriptWithCursor,
  validateScriptSyntax,
} from "../src/services/scripting/scriptSyntax.js";

test("script syntax validation supports the async runner context", () => {
  assert.equal(validateScriptSyntax(`await xterm.sendLine("show version");`), null);
  assert.ok(validateScriptSyntax("if ("));
});

test("compiled scripts receive the xterm API", async () => {
  const calls = [];
  const execute = compileScript(`await xterm.sendLine("show version");`);
  await execute({ sendLine: async (value) => calls.push(value) });
  assert.deepEqual(calls, ["show version"]);
});

test("script formatting preserves top-level await and normalizes layout", async () => {
  const formatted = await formatScript(
    `const value={command:"show version"};await xterm.sendLine(value.command)`,
  );
  assert.equal(
    formatted,
    `const value = { command: "show version" };\nawait xterm.sendLine(value.command);\n`,
  );
});

test("script formatting maps the editor cursor", async () => {
  const source = `const value={command:"show version"};`;
  const result = await formatScriptWithCursor(source, source.indexOf("command"));
  assert.equal(result.formatted, `const value = { command: "show version" };\n`);
  assert.equal(result.formatted.slice(result.cursorOffset).startsWith("command"), true);
});
