import assert from "node:assert/strict";
import test from "node:test";

import unicode11Package from "@xterm/addon-unicode11";
import xtermPackage from "@xterm/xterm";

import { TextProgressBar } from "../src/utils/terminal/addons/trzsz/progress.js";

const { Unicode11Addon } = unicode11Package;
const { Terminal } = xtermPackage;

function createTerminal(t, columns = 80) {
  const terminal = new Terminal({ allowProposedApi: true, cols: columns, rows: 8 });
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";
  t.after(() => terminal.dispose());
  return terminal;
}

function flushTerminal(terminal) {
  return new Promise((resolve) => terminal.write("", resolve));
}

function forceStep(progress, step) {
  progress._lastUpdateTime = 0;
  progress.onStep(step);
}

function visibleLines(terminal) {
  return Array.from({ length: terminal.buffer.active.length }, (_, index) =>
    terminal.buffer.active.getLine(index)?.translateToString(true),
  ).filter(Boolean);
}

async function renderProgress(t, fileName) {
  const terminal = createTerminal(t);
  const progress = new TextProgressBar((output) => terminal.write(output), terminal.cols);
  progress.onNum(1);
  progress.onName(fileName);
  progress.onSize(3000);
  for (const step of [0, 1000, 2000, 3000]) forceStep(progress, step);
  await flushTerminal(terminal);
  return terminal;
}

async function assertProgressStaysOnOneLine(t, fileName) {
  const terminal = await renderProgress(t, fileName);
  const lines = visibleLines(terminal);
  assert.equal(terminal.buffer.active.cursorY, 0, `${fileName}: ${JSON.stringify(lines)}`);
  assert.equal(lines.length, 1, fileName);
  assert.match(lines[0], /100%/, fileName);
  assert.equal(terminal.buffer.active.cursorX, terminal.cols, fileName);
}

const oneLineNames = [
  "demo.bin",
  "测试（大文件）.bin",
  "年度总结（最终版）📊-这是一个用于确认省略号不会拆开Unicode字符的超长文件名.bin",
  "©.bin",
  "e\u0301.bin",
  "𐐀.bin",
  "ꥠꥠ.bin",
  "👨‍👩‍👧‍👦.bin",
];
for (const fileName of oneLineNames) {
  test(`trzsz progress keeps ${JSON.stringify(fileName)} on one full-width line`, async (t) => {
    await assertProgressStaysOnOneLine(t, fileName);
  });
}

test("trzsz progress erases a longer previous frame", async (t) => {
  const terminal = createTerminal(t, 10);
  const progress = new TextProgressBar((output) => terminal.write(output), terminal.cols);
  progress.onNum(2);
  progress.onName("first");
  progress.onSize(100);
  forceStep(progress, 100);
  progress.onName("b");
  progress.onSize(100);
  forceStep(progress, 0);
  await flushTerminal(terminal);

  assert.deepEqual(visibleLines(terminal), ["0%"]);
});

test("trzsz progress cannot wrap a two or three column terminal", async (t) => {
  for (const columns of [2, 3]) {
    const terminal = createTerminal(t, columns);
    const progress = new TextProgressBar((output) => terminal.write(output), columns);
    progress.onNum(1);
    progress.onName("a");
    progress.onSize(100);
    for (const step of [0, 50, 100]) forceStep(progress, step);
    await flushTerminal(terminal);

    assert.equal(terminal.buffer.active.cursorY, 0);
    assert.equal(
      visibleLines(terminal).every((line) => line.length <= columns),
      true,
    );
  }
});

test("trzsz progress starts from a cleared line even when the cursor is not at column zero", async (t) => {
  const terminal = createTerminal(t, 40);
  terminal.write("prompt> ");
  await flushTerminal(terminal);
  const progress = new TextProgressBar((output) => terminal.write(output), terminal.cols);
  progress.onNum(1);
  progress.onName("demo.bin");
  progress.onSize(100);
  progress.onStep(0);
  await flushTerminal(terminal);

  const lines = visibleLines(terminal);
  assert.equal(lines.length, 1, JSON.stringify(lines));
  assert.match(lines[0], /0%/);
  assert.equal(lines[0].includes("prompt>"), false);
  assert.equal(terminal.buffer.active.cursorY, 0);
});

test("trzsz tmux progress also erases a longer previous frame", async (t) => {
  const terminal = createTerminal(t, 10);
  const progress = new TextProgressBar((output) => terminal.write(output), terminal.cols, 10);
  progress.onNum(2);
  progress.onName("first");
  progress.onSize(100);
  forceStep(progress, 100);
  progress.onName("b");
  progress.onSize(100);
  forceStep(progress, 50);
  await flushTerminal(terminal);

  assert.deepEqual(visibleLines(terminal), ["50%"]);
});

test("trzsz progress renders file name controls as visible text", async (t) => {
  const samples = [
    ["bad\nname", "\\n"],
    ["a\tb", "\\t"],
    ["a\rname", "\\r"],
    ["a\bname", "\\b"],
    ["a\x1b[2Jname", "\\x1b"],
    ["a\u009b2Jname", "\\x9b"],
  ];
  for (const [fileName, escaped] of samples) {
    const terminal = createTerminal(t, 40);
    terminal.write("keep\r\n");
    const progress = new TextProgressBar((output) => terminal.write(output), terminal.cols);
    progress.onNum(1);
    progress.onName(fileName);
    progress.onSize(100);
    progress.onStep(0);
    await flushTerminal(terminal);

    const lines = visibleLines(terminal);
    assert.equal(lines[0], "keep", fileName);
    assert.equal(lines.length, 2, `${fileName}: ${JSON.stringify(lines)}`);
    assert.equal(lines[1].includes(escaped), true, `${fileName}: ${lines[1]}`);
    assert.equal(terminal.buffer.active.cursorY, 1, fileName);
  }
});

test("trzsz zero-byte files always publish their final file progress", (t) => {
  t.mock.method(Date, "now", () => 1000);
  const writes = [];
  const progress = new TextProgressBar((output) => writes.push(output), 80);
  progress.onNum(2);
  for (const fileName of ["empty-a", "empty-b"]) {
    progress.onName(fileName);
    progress.onSize(0);
    progress.onStep(0);
    progress.onDone();
  }

  assert.match(writes.at(-1), /\(2\/2\) empty-b/);
  assert.match(writes.at(-1), /100%/);
});

test("trzsz ellipsization does not split combining or ZWJ sequences", () => {
  for (const [fileName, brokenSuffix] of [
    ["👨‍👩‍👧‍👦".repeat(10), "\u200d..."],
    ["e\u0301".repeat(30), "e..."],
  ]) {
    const writes = [];
    const progress = new TextProgressBar((output) => writes.push(output), 50);
    progress.onNum(1);
    progress.onName(fileName);
    progress.onSize(100);
    progress.onStep(0);
    assert.equal(writes.at(-1).includes(brokenSuffix), false, writes.at(-1));
  }
});

test("trzsz resize immediately replaces the old frame", async (t) => {
  const terminal = createTerminal(t, 80);
  const writes = [];
  const progress = new TextProgressBar((output) => {
    writes.push(output);
    terminal.write(output);
  }, terminal.cols);
  progress.onNum(1);
  progress.onName("resize-long-file-name.bin");
  progress.onSize(100);
  progress.onStep(0);
  await flushTerminal(terminal);

  terminal.resize(10, 8);
  progress.setTerminalColumns(10);
  await flushTerminal(terminal);

  assert.equal(writes.length, 2);
  assert.deepEqual(visibleLines(terminal), ["0%"]);
});

test("trzsz a stale queued frame followed by resize cannot leave a wrapped old row", async (t) => {
  const terminal = createTerminal(t, 80);
  const pending = [];
  const progress = new TextProgressBar((output) => pending.push(output), terminal.cols);
  progress.onNum(1);
  progress.onName("demo.bin");
  progress.onSize(3000);
  progress.onStep(0);
  terminal.write(pending[0]);
  await flushTerminal(terminal);

  forceStep(progress, 1000);
  terminal.write(pending[1]);
  terminal.resize(40, 8);
  progress.setTerminalColumns(40);
  forceStep(progress, 2000);
  for (const output of pending.slice(2)) terminal.write(output);
  await flushTerminal(terminal);

  const lines = visibleLines(terminal);
  assert.equal(lines.length, 1, JSON.stringify(lines));
  assert.match(lines[0], /67%/);
  assert.equal(terminal.buffer.active.cursorY, 0);
});
