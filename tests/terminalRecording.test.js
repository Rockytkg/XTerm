import assert from "node:assert/strict";
import test from "node:test";

import unicode11Package from "@xterm/addon-unicode11";
import xtermPackage from "@xterm/xterm";

import { createTerminalRecordingNormalizer } from "../src/stores/terminalRecording.js";

const { Unicode11Addon } = unicode11Package;
const { Terminal } = xtermPackage;

async function xtermLine(input, chunks = [input]) {
  const terminal = new Terminal({ allowProposedApi: true, cols: 80, rows: 5 });
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";
  for (const chunk of chunks) {
    await new Promise((resolve) => terminal.write(chunk, resolve));
  }
  const line = terminal.buffer.active.getLine(0)?.translateToString(true) || "";
  terminal.dispose();
  return line;
}

async function assertMatchesXterm(t, input, chunks = [input]) {
  const normalizer = createTerminalRecordingNormalizer();
  let recorded = "";
  for (const chunk of chunks) recorded += normalizer.normalize(chunk);
  assert.equal(recorded, `${await xtermLine(input, chunks)}\n`, JSON.stringify(input));
}

const cases = [
  "你\tX\r\n",
  "e\u0301\tX\r\n",
  "\u0301\u0301A\tX\r\n",
  "abc\r你\bX\r\n",
  "你A\rX\r\n",
  "你\bX\r\n",
  "你🙂\tX\r\n",
  "👨‍👩‍👧‍👦\tX\r\n",
  "🇨🇳\tX\r\n",
];
for (const input of cases) {
  test(`recording matches xterm for ${JSON.stringify(input)}`, async (t) => {
    await assertMatchesXterm(t, input);
  });
}

test("recording combines a character split from its combining mark", async (t) => {
  await assertMatchesXterm(t, "e\u0301\tX\r\n", ["e", "\u0301\tX\r\n"]);
});

test("recording limits lines by terminal cells without splitting a wide glyph", () => {
  const fitted = createTerminalRecordingNormalizer();
  assert.equal(fitted.normalize(`${"a".repeat(8190)}你\r\n`), `${"a".repeat(8190)}你\n`);

  const clipped = createTerminalRecordingNormalizer();
  assert.equal(clipped.normalize(`${"a".repeat(8191)}你\r\n`), `${"a".repeat(8191)}\n`);
});
