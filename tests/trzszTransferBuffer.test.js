import assert from "node:assert/strict";
import test from "node:test";
import { TransferBuffer } from "../src/utils/terminal/addons/trzsz/terminalBuffer.js";

function utf8(value) {
  return new TextEncoder().encode(value);
}

test("readLine returns the line without the newline", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("#SUCC:42\n"));
  assert.equal(await buffer.readLine(), "#SUCC:42");
});

test("readLine joins chunks split mid-line", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("#SU"));
  buffer.add(utf8("CC:42"));
  buffer.add(utf8("\nrest\n"));
  assert.equal(await buffer.readLine(), "#SUCC:42");
  assert.equal(await buffer.readLine(), "rest");
});

test("readLine rejects on interrupt byte", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("abc\x03def\n"));
  await assert.rejects(buffer.readLine(), /Interrupted/);
});

test("readBinary reads exactly the requested size across chunks", async () => {
  const buffer = new TransferBuffer();
  buffer.add(Uint8Array.from([1, 2, 3]));
  buffer.add(Uint8Array.from([4, 5, 6, 7]));
  assert.deepEqual(Array.from(await buffer.readBinary(5)), [1, 2, 3, 4, 5]);
  assert.deepEqual(Array.from(await buffer.readBinary(2)), [6, 7]);
});

test("readWindowsLine stops at the ! marker and keeps protocol bytes", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("#SUCC:42!\n"));
  assert.equal(await buffer.readWindowsLine(), "#SUCC:42");
});

test("readWindowsLine strips VT escape sequences", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("#SU\x1b[2KCC:42!\n"));
  assert.equal(await buffer.readWindowsLine(), "#SUCC:42");
});

test("stop rejects pending and future reads", async () => {
  const buffer = new TransferBuffer();
  const pending = buffer.readLine();
  buffer.stop();
  await assert.rejects(pending, /Stopped/);
  buffer.add(utf8("ignored\n"));
  await assert.rejects(buffer.readLine(), /Stopped/);
});

test("drain clears queued data", async () => {
  const buffer = new TransferBuffer();
  buffer.add(utf8("old\n"));
  buffer.drain();
  buffer.add(utf8("new\n"));
  assert.equal(await buffer.readLine(), "new");
});
