import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyTerminalOutputPayload,
  createTerminalOutputByteDecoder,
  decodeTerminalOutputBytes,
} from "../src/utils/terminalOutputPayload.js";

function toBase64(bytes) {
  return btoa(String.fromCharCode(...bytes));
}

function outputState(overrides = {}) {
  return {
    sessionId: "session-1",
    sessionChannelId: 7,
    terminalOutputCursor: 0,
    terminalOutputCursorSessionId: "session-1",
    ...overrides,
  };
}

test("raw output trims bytes already covered by the cursor", () => {
  const decision = classifyTerminalOutputPayload(
    {
      kind: "bytes",
      sessionId: "session-1",
      channelId: 7,
      dataBase64: toBase64(Uint8Array.from([65, 66, 67, 68])),
      startOffset: 10,
      endOffset: 14,
    },
    outputState({ terminalOutputCursor: 12 }),
  );

  assert.equal(decision.kind, "raw");
  // 裁剪后直接携带字节，不再做 decode→encode 往返。
  assert.deepEqual(decision.normalized.bytes, Uint8Array.from([67, 68]));
  assert.equal(decision.normalized.endOffset, 14);
});

test("large raw output trimming decodes base64 exactly once", () => {
  const bytes = new Uint8Array(40_000).fill(65);
  const decision = classifyTerminalOutputPayload(
    {
      kind: "bytes",
      sessionId: "session-1",
      channelId: 7,
      dataBase64: Buffer.from(bytes).toString("base64"),
      startOffset: 0,
      endOffset: bytes.length,
    },
    outputState({ terminalOutputCursor: 1 }),
  );

  assert.equal(decision.kind, "raw");
  const trimmed = decision.normalized.bytes;
  assert.equal(trimmed.length, bytes.length - 1);
  assert.equal(trimmed[0], 65);
  assert.equal(trimmed.at(-1), 65);
});

test("text replay trimming respects UTF-8 byte offsets without splitting code points", () => {
  const data = "A你🙂Z";
  const payload = {
    kind: "text",
    sessionId: "session-1",
    channelId: 7,
    data,
    startOffset: 10,
    endOffset: 19,
  };

  for (const [cursor, expected] of [
    [12, "你🙂Z"], // byte offset lands inside 你
    [14, "🙂Z"], // exact boundary after 你
    [15, "🙂Z"], // byte offset lands inside the surrogate-pair emoji
    [18, "Z"], // exact boundary before the final ASCII character
  ]) {
    const decision = classifyTerminalOutputPayload(
      payload,
      outputState({ terminalOutputCursor: cursor }),
    );
    assert.equal(decision.kind, "text");
    assert.equal(decision.normalized.data, expected);
  }
});

test("raw output without the required range is rejected", () => {
  const decision = classifyTerminalOutputPayload(
    {
      kind: "bytes",
      sessionId: "session-1",
      channelId: 7,
      dataBase64: toBase64(Uint8Array.from([65, 66, 67])),
    },
    outputState(),
  );

  assert.equal(decision.kind, "ignore");
  assert.equal(decision.normalized, null);
});

test("terminal output channel rejects control-plane payloads", () => {
  for (const kind of ["status", "error", "metrics"]) {
    const decision = classifyTerminalOutputPayload(
      { kind, sessionId: "session-1", channelId: 7 },
      outputState(),
    );

    assert.equal(decision.kind, "ignore");
    assert.equal(decision.normalized, null);
  }
});

test("raw output without trimming passes the base64 payload through unchanged", () => {
  const dataBase64 = toBase64(Uint8Array.from([65, 66, 67, 68]));
  const decision = classifyTerminalOutputPayload(
    {
      kind: "bytes",
      sessionId: "session-1",
      channelId: 7,
      dataBase64,
      startOffset: 10,
      endOffset: 14,
    },
    outputState({ terminalOutputCursor: 0 }),
  );

  assert.equal(decision.kind, "raw");
  // trimBytes === 0 时不做 decode→encode 往返，输出与输入恒等。
  assert.equal(decision.normalized.dataBase64, dataBase64);
  assert.equal(decision.normalized.endOffset, 14);
});

test("raw output already fully covered by the cursor is ignored", () => {
  const decision = classifyTerminalOutputPayload(
    {
      kind: "bytes",
      sessionId: "session-1",
      channelId: 7,
      dataBase64: toBase64(Uint8Array.from([65, 66])),
      startOffset: 10,
      endOffset: 12,
    },
    outputState({ terminalOutputCursor: 12 }),
  );

  assert.equal(decision.kind, "ignore");
  assert.equal(decision.normalized, null);
});

test("streaming byte decoder preserves UTF-8 characters split across payloads", () => {
  const decoder = createTerminalOutputByteDecoder();
  const bytes = new TextEncoder().encode("你");

  assert.equal(decoder.decode(toBase64(bytes.subarray(0, 2))), "");
  assert.equal(decoder.decode(toBase64(bytes.subarray(2))), "你");
});

test("decoded raw bytes can be reused by the streaming text decoder", () => {
  const decoder = createTerminalOutputByteDecoder();
  const bytes = new TextEncoder().encode("你");

  assert.deepEqual(decodeTerminalOutputBytes(toBase64(bytes)), bytes);
  assert.equal(decoder.decodeBytes(bytes.subarray(0, 2)), "");
  assert.equal(decoder.decodeBytes(bytes.subarray(2)), "你");
});

test("reset discards an incomplete byte sequence from the previous session", () => {
  const decoder = createTerminalOutputByteDecoder();
  const bytes = new TextEncoder().encode("你");

  assert.equal(decoder.decode(toBase64(bytes.subarray(0, 2))), "");
  decoder.reset();
  assert.equal(decoder.decode(toBase64(new TextEncoder().encode("A"))), "A");
});
