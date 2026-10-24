import assert from "node:assert/strict";
import test from "node:test";

import {
  getTerminalCodePointWidth,
  getTerminalTextWidth,
} from "../src/utils/terminal/unicodeWidth.js";

test("terminal widths match the Unicode 11 provider", () => {
  const samples = [
    ["A", 1],
    ["你", 2],
    ["（", 2],
    ["ꥠ", 2],
    ["📊", 2],
    ["𐐀", 1],
    ["𝐀", 1],
    ["©", 1],
    ["\u0301", 0],
    ["\u200d", 0],
    ["\ufe0f", 0],
    ["🏽", 2],
    ["🇨", 1],
  ];
  for (const [character, expected] of samples) {
    assert.equal(getTerminalCodePointWidth(character.codePointAt(0)), expected, character);
  }
});

test("terminal text widths combine code points like xterm", () => {
  const samples = [
    ["e\u0301", 1],
    ["👨‍👩‍👧‍👦", 8],
    ["👍🏽", 4],
    ["🇨🇳", 2],
    ["✈️", 1],
    ["\u0301A", 2],
    ["\u0301\u0301A", 3],
  ];
  for (const [text, expected] of samples) {
    assert.equal(getTerminalTextWidth(text), expected, text);
  }
});
