import assert from "node:assert/strict";
import test from "node:test";
import { createWheelAccumulator } from "../src/utils/wheelAccumulator.js";

function wheelEvent(deltaY, deltaMode = 0) {
  return { deltaY, deltaMode };
}

test("accumulates pixel deltas and fires send once per threshold", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({ send: (direction) => sent.push(direction) });

  accumulator.feed(wheelEvent(40));
  accumulator.feed(wheelEvent(40));
  assert.deepEqual(sent, []);
  accumulator.feed(wheelEvent(40));
  assert.deepEqual(sent, [1]);

  // 余量 20px 保留，继续累积到阈值再次触发。
  accumulator.feed(wheelEvent(80));
  assert.deepEqual(sent, [1, 1]);
});

test("large delta fires multiple steps and keeps the remainder", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({ send: (direction) => sent.push(direction) });

  accumulator.feed(wheelEvent(250));
  assert.deepEqual(sent, [1, 1]);
  accumulator.feed(wheelEvent(-60));
  // 方向反转先清零 50px 余量，-60 未达阈值。
  assert.deepEqual(sent, [1, 1]);
});

test("direction reversal drops the accumulated remainder", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({ send: (direction) => sent.push(direction) });

  accumulator.feed(wheelEvent(90));
  accumulator.feed(wheelEvent(-90));
  assert.deepEqual(sent, []);
  // 旧 +90 余量已丢弃：-90 + -20 恰好过一次阈值，只发一次。
  accumulator.feed(wheelEvent(-20));
  assert.deepEqual(sent, [-1]);
});

test("deltaMode 1 (lines) is converted with 19px per line", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({ send: (direction) => sent.push(direction) });

  // 5 行 × 19px = 95px，未达阈值。
  accumulator.feed(wheelEvent(5, 1));
  assert.deepEqual(sent, []);
  // 再 1 行 = 114px，触发一次后余 14px。
  accumulator.feed(wheelEvent(1, 1));
  assert.deepEqual(sent, [1]);
  accumulator.feed(wheelEvent(5, 1));
  assert.deepEqual(sent, [1, 1]);
});

test("deltaMode 2 (pages) uses getPageHeightPx", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({
    send: (direction) => sent.push(direction),
    getPageHeightPx: () => 300,
  });

  accumulator.feed(wheelEvent(1, 2));
  assert.deepEqual(sent, [1, 1, 1]);
});

test("failed send clears the accumulator and stops", () => {
  const sent = [];
  let failNext = false;
  const accumulator = createWheelAccumulator({
    send: (direction) => {
      if (failNext) return false;
      sent.push(direction);
      return true;
    },
  });

  failNext = true;
  accumulator.feed(wheelEvent(250));
  assert.deepEqual(sent, []);
  failNext = false;

  // 余量已清零：40+40 不触发，再加 40 才到阈值。
  accumulator.feed(wheelEvent(40));
  accumulator.feed(wheelEvent(40));
  assert.deepEqual(sent, []);
  accumulator.feed(wheelEvent(40));
  assert.deepEqual(sent, [1]);
});

test("reset clears the accumulated remainder", () => {
  const sent = [];
  const accumulator = createWheelAccumulator({ send: (direction) => sent.push(direction) });

  accumulator.feed(wheelEvent(90));
  accumulator.reset();
  accumulator.feed(wheelEvent(90));
  assert.deepEqual(sent, []);
});
