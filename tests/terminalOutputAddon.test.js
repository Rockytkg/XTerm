import assert from "node:assert/strict";
import test from "node:test";
import { TerminalOutputAddon } from "../src/utils/terminal/addons/output/TerminalOutputAddon.js";

// autoParse 模式模拟 xterm：write 的解析回调在后续事件循环回合触发。
// 关闭后回调由测试手动触发，用于断言收束时机。
function createTerminal({ autoParse = true } = {}) {
  const callbacks = [];
  const writes = [];
  return {
    callbacks,
    writes,
    write(data, callback) {
      writes.push(data);
      if (!callback) return;
      if (autoParse) setTimeout(callback, 0);
      else callbacks.push(callback);
    },
    flushCallbacks() {
      while (callbacks.length) callbacks.shift()();
    },
  };
}

function createAddon(options = {}) {
  return new TerminalOutputAddon({
    isDisposed: () => false,
    writeChunkChars: 4096,
    ...options,
  });
}

// 写入经 writeLock 微任务链推进：先让出事件循环让 write 调用落位。
function settleWrites() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

test("queued output is written through immediately without any scheduler", async () => {
  const terminal = createTerminal();
  const addon = createAddon();
  addon.activate(terminal);

  // 数据通路无定时器/RAF 等待：queue 返回后缓冲区即刻排空进入写入链。
  addon.queue("first");
  assert.equal(addon._buffer, "");
  addon.queue(" second");
  await addon.waitForFlush();

  assert.deepEqual(terminal.writes, ["first", " second"]);
});

test("rapid sequential batches preserve order", async () => {
  const terminal = createTerminal();
  const addon = createAddon();
  addon.activate(terminal);

  for (let index = 0; index < 50; index += 1) addon.queue(`chunk${index};`);
  await addon.waitForFlush();

  assert.equal(
    terminal.writes.join(""),
    Array.from({ length: 50 }, (_, index) => `chunk${index};`).join(""),
  );
});

test("a flush writes fixed-size chunks and settles on the last chunk's parse", async () => {
  const terminal = createTerminal({ autoParse: false });
  const addon = createAddon();
  addon.activate(terminal);

  addon.queue("x".repeat(20_000));
  await settleWrites();

  // 20_000 字符按 4096 分片：4 个整片 + 余量；分片间不 await，一次冲刷内
  // 全部入队。
  assert.equal(terminal.writes.length, 5);
  assert.equal(terminal.writes[0].length, 4096);
  assert.equal(terminal.writes.at(-1).length, 20_000 - 4 * 4096);

  let settled = false;
  const flushed = addon.waitForFlush().then(() => {
    settled = true;
  });
  // 前面的分片解析完成还不收束；最后一片（WriteBuffer FIFO，代表本批全部
  // 解析完成）触发后才收束。
  terminal.callbacks.slice(0, -1).forEach((callback) => callback());
  await Promise.resolve();
  assert.equal(settled, false);
  terminal.callbacks.at(-1)();
  await flushed;
  assert.equal(settled, true);
});

test("drop releases a write whose callback never fires", async () => {
  const terminal = createTerminal({ autoParse: false });
  const addon = createAddon();
  addon.activate(terminal);

  addon.queue("pending");
  await settleWrites();
  assert.deepEqual(terminal.writes, ["pending"]);

  // 模拟终端已销毁：xterm 会静默丢弃队列中的 write 回调，drop 必须主动收束。
  const flushed = addon.waitForFlush();
  addon.drop();
  await flushed;

  addon.queue("after");
  const second = addon.waitForFlush();
  await settleWrites();
  terminal.flushCallbacks();
  await second;
  assert.deepEqual(terminal.writes, ["pending", "after"]);
});

test("does not retain output segments while recording is disabled", async () => {
  const terminal = createTerminal();
  const addon = createAddon({ isRecordingActive: () => false });
  addon.activate(terminal);

  addon.queue("ordinary output", { recordable: true });
  assert.equal(addon._segments.length, 0);
  await addon.waitForFlush();
  assert.equal(terminal.writes.join(""), "ordinary output");
});

test("recordable output stays aligned when a batch contains non-recordable text", async () => {
  const terminal = createTerminal();
  const recorded = [];
  const addon = createAddon({
    isRecordingActive: () => true,
    onRecordChunk: (chunk) => recorded.push(chunk),
  });
  addon.activate(terminal);

  addon.queue("status text", { recordable: false });
  addon.queue("shell output", { recordable: true });
  await addon.waitForFlush();

  assert.deepEqual(recorded, ["shell output"]);
  assert.equal(terminal.writes.join(""), "status textshell output");
});

test("does not allocate segments while recording is inactive", async () => {
  const terminal = createTerminal();
  const addon = createAddon({
    isRecordingActive: () => false,
    onRecordChunk: () => {},
  });
  addon.activate(terminal);

  addon.queue("output", { recordable: true });
  assert.equal(addon._segments.length, 0);
  await addon.waitForFlush();
  assert.equal(terminal.writes.join(""), "output");
});
