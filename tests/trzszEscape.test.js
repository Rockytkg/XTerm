import assert from "node:assert/strict";
import test from "node:test";
import {
  escapeCodes,
  escapeData,
  unescapeData,
} from "../src/utils/terminal/addons/trzsz/escape.js";

function roundTrip(data, escapeChars) {
  const codes = escapeCodes(escapeChars);
  return unescapeData(escapeData(data, codes), codes);
}

test("no escape chars returns input unchanged", () => {
  const data = Uint8Array.from([0, 1, 2, 255, 128]);
  const codes = escapeCodes([]);
  assert.equal(escapeData(data, codes), data);
  assert.equal(unescapeData(data, codes), data);
});

test("escapes and unescapes configured bytes", () => {
  const escapeChars = [
    ["\x03", "\x7f1"],
    ["\x1d", "\x7f2"],
  ];
  const data = Uint8Array.from([1, 2, 3, 4, 0x1d, 5]);
  const codes = escapeCodes(escapeChars);
  const escaped = escapeData(data, codes);
  assert.deepEqual(Array.from(escaped), [1, 2, 0x7f, 0x31, 4, 0x7f, 0x32, 5]);
  assert.deepEqual(Array.from(unescapeData(escaped, codes)), Array.from(data));
});

test("escape lead byte round-trips when it is itself escaped", () => {
  const escapeChars = [
    ["\x03", "\x7f1"],
    ["\x7f", "\x7f7"],
  ];
  const data = Uint8Array.from([0x7f, 0x31, 3]);
  assert.deepEqual(Array.from(roundTrip(data, escapeChars)), Array.from(data));
});

test("round-trips all 256 byte values", () => {
  const escapeChars = [
    ["\x00", "\xee0"],
    ["\x0a", "\xee1"],
    ["\xff", "\xee2"],
  ];
  const data = Uint8Array.from({ length: 256 }, (_, index) => index);
  assert.deepEqual(Array.from(roundTrip(data, escapeChars)), Array.from(data));
});

test("trailing escape lead byte without pair stays literal", () => {
  const escapeChars = [["\x03", "\x7f1"]];
  const codes = escapeCodes(escapeChars);
  const data = Uint8Array.from([1, 0x7f]);
  assert.deepEqual(Array.from(unescapeData(data, codes)), [1, 0x7f]);
});

test("unknown pair stays literal during unescape", () => {
  const escapeChars = [["\x03", "\x7f1"]];
  const codes = escapeCodes(escapeChars);
  const data = Uint8Array.from([0x7f, 0x99, 2]);
  assert.deepEqual(Array.from(unescapeData(data, codes)), [0x7f, 0x99, 2]);
});
