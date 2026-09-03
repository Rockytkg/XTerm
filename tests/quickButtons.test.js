import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_COLOR, normalizeQuickButton } from "../src/utils/quickButtons.js";
import { expandTerminalEscapes, splitTerminalSendContent } from "../src/utils/terminalEscapes.js";

test("normalizeQuickButton rejects entries without id or name", () => {
  assert.equal(normalizeQuickButton(null), null);
  assert.equal(normalizeQuickButton({ name: "x" }), null);
  assert.equal(normalizeQuickButton({ id: "a", name: "   " }), null);
});

test("normalizeQuickButton trims name and falls back to send/default color", () => {
  const button = normalizeQuickButton({ id: "a", name: " 重启 " });
  assert.deepEqual(button, {
    id: "a",
    name: "重启",
    type: "send",
    value: "",
    color: DEFAULT_COLOR,
  });
});

test("normalizeQuickButton keeps script type and explicit color", () => {
  const button = normalizeQuickButton({
    id: "b",
    name: "部署",
    type: "script",
    value: "script-1",
    color: "#ef6b73",
  });
  assert.equal(button.type, "script");
  assert.equal(button.value, "script-1");
  assert.equal(button.color, "#ef6b73");
});

test("expandTerminalEscapes expands common control sequences", () => {
  assert.equal(expandTerminalEscapes("ls\\n"), "ls\n");
  assert.equal(expandTerminalEscapes("\\r\\t\\0"), "\r\t\0");
  assert.equal(expandTerminalEscapes("\\e[31m"), "\x1b[31m");
  assert.equal(expandTerminalEscapes("\\x1b[0m"), "\x1b[0m");
});

test("expandTerminalEscapes expands unicode forms and escaped backslash", () => {
  assert.equal(expandTerminalEscapes("\\u0041"), "A");
  assert.equal(expandTerminalEscapes("\\u{1F600}"), "\u{1F600}");
  assert.equal(expandTerminalEscapes("a\\\\nb"), "a\\nb");
});

test("expandTerminalEscapes leaves unknown sequences and non-strings intact", () => {
  assert.equal(expandTerminalEscapes("a\\qb"), "a\\qb");
  assert.equal(expandTerminalEscapes(undefined), "");
});

test("splitTerminalSendContent splits text around delay markers", () => {
  assert.deepEqual(splitTerminalSendContent("enable\\n\\d300\\nconfig"), [
    { type: "text", text: "enable\\n" },
    { type: "delay", ms: 300 },
    { type: "text", text: "\\nconfig" },
  ]);
});

test("splitTerminalSendContent keeps adjacent delays and bare text", () => {
  assert.deepEqual(splitTerminalSendContent("\\d100\\d200"), [
    { type: "delay", ms: 100 },
    { type: "delay", ms: 200 },
  ]);
  assert.deepEqual(splitTerminalSendContent("reload\\n"), [{ type: "text", text: "reload\\n" }]);
  assert.deepEqual(splitTerminalSendContent(""), []);
});

test("splitTerminalSendContent treats escaped backslash before d as literal", () => {
  // \\d500 展开后应发送字面 \d500，而不是暂停
  const [segment] = splitTerminalSendContent("\\\\d500");
  assert.deepEqual(segment, { type: "text", text: "\\\\d500" });
  assert.equal(expandTerminalEscapes(segment.text), "\\d500");
  // 三个反斜杠：字面 \ 之后仍是有效暂停
  assert.deepEqual(splitTerminalSendContent("\\\\\\d250"), [
    { type: "text", text: "\\\\" },
    { type: "delay", ms: 250 },
  ]);
});

test("splitTerminalSendContent leaves \\d without digits as plain text", () => {
  assert.deepEqual(splitTerminalSendContent("a\\db"), [{ type: "text", text: "a\\db" }]);
});
