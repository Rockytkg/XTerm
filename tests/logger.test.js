import assert from "node:assert/strict";
import test from "node:test";
import {
  createLogger,
  getGlobalLogLevel,
  setGlobalLogLevel,
  summarizeValue,
} from "../src/utils/logger.js";

// node --test 环境下 import.meta.env 为 undefined，logger 处于生产模式
// （LOGGING_ENABLED=false），console 输出关闭，error/warn 仅在存在
// window 时转发后端——此处验证纯函数与级别门控逻辑。

test("setGlobalLogLevel accepts known levels", () => {
  assert.equal(setGlobalLogLevel("debug"), "debug");
  assert.equal(getGlobalLogLevel(), "debug");
  assert.equal(setGlobalLogLevel("trace"), "trace");
  assert.equal(getGlobalLogLevel(), "trace");
});

test("setGlobalLogLevel falls back to the default for unknown levels", () => {
  // 非 DEV 环境默认级别为 error
  assert.equal(setGlobalLogLevel("verbose"), "error");
  assert.equal(setGlobalLogLevel(""), "error");
  assert.equal(setGlobalLogLevel(undefined), "error");
});

test("summarizeValue truncates long strings", () => {
  const long = "x".repeat(500);
  const summary = summarizeValue(long);
  assert.equal(summary.length, 160);
  assert.ok(summary.endsWith("..."));
});

test("summarizeValue summarizes errors structurally", () => {
  const summary = summarizeValue(new TypeError("boom"));
  assert.deepEqual(summary, {
    name: "TypeError",
    message: "boom",
    code: undefined,
    detail: undefined,
  });
});

test("summarizeValue is cycle-safe", () => {
  const value = { self: null };
  value.self = value;
  const summary = summarizeValue(value);
  assert.equal(summary.self, "[Circular]");
});

test("summarizeValue caps arrays and object keys", () => {
  const arraySummary = summarizeValue([1, 2, 3, 4, 5, 6, 7]);
  assert.equal(arraySummary.length, 6);
  assert.equal(arraySummary[5], "...(2 more)");

  const objectSummary = summarizeValue(
    Object.fromEntries(Array.from({ length: 12 }, (_, index) => [`k${index}`, index])),
  );
  assert.equal(Object.keys(objectSummary).length, 9);
  assert.equal(objectSummary.__truncated, "4 more keys");
});

test("summarizeValue bounds nesting depth", () => {
  const summary = summarizeValue({ a: { b: { c: { d: 1 } } } });
  assert.equal(summary.a.b, "[Object]");
});

test("scoped logger children extend the parent scope", () => {
  const parent = createLogger("frontend.test");
  const child = parent.child("child-scope");
  assert.equal(child.scope, "frontend.test.child-scope");
  const contextual = parent.withContext({ requestId: "r1" });
  assert.equal(contextual.scope, "frontend.test");
  assert.equal(contextual.context.requestId, "r1");
});

test("emit in non-DEV without window does not throw or forward", () => {
  const logger = createLogger("frontend.test.emit");
  setGlobalLogLevel("error");
  logger.error("event.failed", { detail: "x" });
  logger.warn("event.degraded");
  logger.info("event.invisible");
});
