import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyTerminalOutputPayload,
  createTerminalOutputByteDecoder,
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
  assert.equal(atob(decision.normalized.dataBase64), "CD");
  assert.equal(decision.normalized.endOffset, 14);
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

test("reset discards an incomplete byte sequence from the previous session", () => {
  const decoder = createTerminalOutputByteDecoder();
  const bytes = new TextEncoder().encode("你");

  assert.equal(decoder.decode(toBase64(bytes.subarray(0, 2))), "");
  decoder.reset();
  assert.equal(decoder.decode(toBase64(new TextEncoder().encode("A"))), "A");
});
