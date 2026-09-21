import assert from "node:assert/strict";
import test from "node:test";
import {
  RDP_CLIENT_MESSAGE,
  RDP_SERVER_MESSAGE,
  decodeServerMessage,
  encodeClipboardText,
  encodeKey,
  encodePointerButton,
  encodePointerMove,
  encodeResizeRequest,
  encodeWheel,
} from "../src/utils/rdpCodec.js";
import { scancodeForKeyEvent } from "../src/utils/rdpScancodeMap.js";

function bytes(...values) {
  return new Uint8Array(values).buffer;
}

test("decodes Hello with little-endian dimensions", () => {
  const message = decodeServerMessage(bytes(0x01, 0x80, 0x07, 0x38, 0x04));
  assert.deepEqual(message, { type: RDP_SERVER_MESSAGE.HELLO, width: 1920, height: 1080 });
});

test("decodes Resize", () => {
  const message = decodeServerMessage(bytes(0x03, 0x00, 0x05, 0x00, 0x03));
  assert.deepEqual(message, { type: RDP_SERVER_MESSAGE.RESIZE, width: 1280, height: 768 });
});

test("decodes Frame with multiple dirty rects sharing the buffer", () => {
  // rect1: (0,0) 2x1，8 字节像素；rect2: (5,6) 1x1，4 字节像素。
  const buffer = bytes(
    0x02,
    0x02,
    0x00, // count = 2
    0x00,
    0x00,
    0x00,
    0x00,
    0x02,
    0x00,
    0x01,
    0x00,
    1,
    2,
    3,
    4,
    5,
    6,
    7,
    8,
    0x05,
    0x00,
    0x06,
    0x00,
    0x01,
    0x00,
    0x01,
    0x00,
    9,
    10,
    11,
    12,
  );
  const message = decodeServerMessage(buffer);
  assert.equal(message.type, RDP_SERVER_MESSAGE.FRAME);
  assert.equal(message.rects.length, 2);
  const [first, second] = message.rects;
  assert.deepEqual(
    { x: first.x, y: first.y, width: first.width, height: first.height },
    { x: 0, y: 0, width: 2, height: 1 },
  );
  assert.deepEqual([...first.pixels], [1, 2, 3, 4, 5, 6, 7, 8]);
  assert.deepEqual(
    { x: second.x, y: second.y, width: second.width, height: second.height },
    { x: 5, y: 6, width: 1, height: 1 },
  );
  assert.deepEqual([...second.pixels], [9, 10, 11, 12]);
});

test("rejects truncated frames instead of reading out of bounds", () => {
  // 声明 2x2 rect（需要 16 字节像素）但只给了 4 字节。
  const buffer = bytes(0x02, 0x01, 0x00, 0, 0, 0, 0, 2, 0, 2, 0, 1, 2, 3, 4);
  assert.equal(decodeServerMessage(buffer), null);
  assert.equal(decodeServerMessage(bytes(0x02)), null);
});

test("decodes ClipboardText with utf8 payload", () => {
  const text = "你好 RDP";
  const payload = new TextEncoder().encode(text);
  const buffer = new ArrayBuffer(5 + payload.length);
  const view = new DataView(buffer);
  view.setUint8(0, 0x04);
  view.setUint32(1, payload.length, true);
  new Uint8Array(buffer, 5).set(payload);
  const message = decodeServerMessage(buffer);
  assert.deepEqual(message, { type: RDP_SERVER_MESSAGE.CLIPBOARD_TEXT, text });
});

test("decodes Disconnect reason", () => {
  const payload = new TextEncoder().encode("idle timeout");
  const buffer = new ArrayBuffer(3 + payload.length);
  const view = new DataView(buffer);
  view.setUint8(0, 0x05);
  view.setUint16(1, payload.length, true);
  new Uint8Array(buffer, 3).set(payload);
  const message = decodeServerMessage(buffer);
  assert.deepEqual(message, { type: RDP_SERVER_MESSAGE.DISCONNECT, reason: "idle timeout" });
});

test("returns null for unknown or empty messages", () => {
  assert.equal(decodeServerMessage(bytes()), null);
  assert.equal(decodeServerMessage(bytes(0x7f, 1, 2)), null);
});

test("encodes pointer move / button / wheel", () => {
  assert.deepEqual([...new Uint8Array(encodePointerMove(100, 200))], [0x01, 100, 0, 200, 0]);
  assert.deepEqual([...new Uint8Array(encodePointerButton(1, true))], [0x02, 1, 1]);
  assert.deepEqual([...new Uint8Array(encodePointerButton(2, false))], [0x02, 2, 0]);
  // 正 = 向上；负数以 i16 小端编码。
  assert.deepEqual([...new Uint8Array(encodeWheel(1))], [0x03, 1, 0]);
  assert.deepEqual([...new Uint8Array(encodeWheel(-3))], [0x03, 0xfd, 0xff]);
});

test("encodes key with extended flag", () => {
  assert.deepEqual([...new Uint8Array(encodeKey(0x1e, true, 0))], [0x04, 0x1e, 0x00, 1, 0]);
  assert.deepEqual([...new Uint8Array(encodeKey(0x53, false, 1))], [0x04, 0x53, 0x00, 0, 1]);
});

test("encodes resize request", () => {
  assert.deepEqual(
    [...new Uint8Array(encodeResizeRequest(1440, 900))],
    [0x05, 0xa0, 0x05, 0x84, 0x03],
  );
});

test("encodes clipboard text with utf8 length prefix", () => {
  const encoded = new Uint8Array(encodeClipboardText("剪"));
  const payload = new TextEncoder().encode("剪");
  assert.equal(encoded[0], RDP_CLIENT_MESSAGE.CLIPBOARD_TEXT);
  assert.equal(new DataView(encoded.buffer).getUint32(1, true), payload.length);
  assert.deepEqual([...encoded.slice(5)], [...payload]);
});

test("scancode map covers letters, modifiers and extended navigation keys", () => {
  assert.deepEqual(scancodeForKeyEvent({ code: "KeyA" }), { scancode: 0x1e, extended: 0 });
  assert.deepEqual(scancodeForKeyEvent({ code: "Digit0" }), { scancode: 0x0b, extended: 0 });
  assert.deepEqual(scancodeForKeyEvent({ code: "ControlRight" }), { scancode: 0x1d, extended: 1 });
  assert.deepEqual(scancodeForKeyEvent({ code: "ArrowUp" }), { scancode: 0x48, extended: 1 });
  assert.deepEqual(scancodeForKeyEvent({ code: "Delete" }), { scancode: 0x53, extended: 1 });
  assert.deepEqual(scancodeForKeyEvent({ code: "F12" }), { scancode: 0x58, extended: 0 });
  assert.deepEqual(scancodeForKeyEvent({ code: "MetaLeft" }), { scancode: 0x5b, extended: 1 });
  assert.equal(scancodeForKeyEvent({ code: "Unidentified" }), null);
  assert.equal(scancodeForKeyEvent(null), null);
});
