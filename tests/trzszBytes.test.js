import assert from "node:assert/strict";
import test from "node:test";
import { decodeBuffer, encodeBuffer } from "../src/utils/terminal/addons/trzsz/bytes.js";

test("encodeBuffer/decodeBuffer round-trips UTF-8 protocol values", () => {
  const value = "hello 世界 ".repeat(1000);
  assert.equal(new TextDecoder().decode(decodeBuffer(encodeBuffer(value))), value);
  assert.equal(decodeBuffer(encodeBuffer("")).length, 0);
});

test("decodeBuffer rejects payloads that expand past the protocol line limit", () => {
  // 20MB 全零数据压缩后极小；必须在解压过程中拒绝，而不是完整展开后才发现。
  const bomb = encodeBuffer("A".repeat(20 * 1024 * 1024));
  assert.throws(() => decodeBuffer(bomb), /too large/);
});

test("decodeBuffer rejects invalid payloads", () => {
  assert.throws(() => decodeBuffer("!!!not-base64!!!"));
});
