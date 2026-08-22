import assert from "node:assert/strict";
import test from "node:test";
import {
  SCRIPT_RUN_STATUS,
  runScript,
  scriptRuns,
  stopScript,
  stripAnsi,
} from "../src/services/scripting/scriptRunner.js";
import {
  publishTerminalOutput,
  registerRecordingBridge,
  registerScriptBridge,
} from "../src/services/scripting/bridges.js";
import {
  requestScriptPrompt,
  resolveScriptPrompt,
  scriptPrompt,
} from "../src/services/scripting/scriptPromptController.js";

const TARGET = "test-session";

// 模拟 ScriptBridgeAddon 的测试桩：记录 send 的文本，提供输出订阅。
function createFakeBridge() {
  const listeners = new Set();
  return {
    sent: [],
    bufferText: "",
    send(data) {
      this.sent.push(data);
      return true;
    },
    getScreenText: () => "",
    getBufferText() {
      return this.bufferText;
    },
    notifyOutput(data) {
      for (const listener of [...listeners]) listener(data);
    },
    onOutput(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

// 模拟会话记录控制器的测试桩：start 返回值决定"用户选路 / 取消"。
function createFakeRecording(startResult = "/tmp/recording.txt") {
  return {
    active: false,
    path: "",
    async start() {
      if (!startResult) return "";
      this.active = true;
      this.path = startResult;
      return this.path;
    },
    async stop() {
      this.active = false;
      const path = this.path;
      this.path = "";
      return path;
    },
    isActive() {
      return this.active;
    },
  };
}

let bridge = null;
let unregister = null;

function setup() {
  bridge = createFakeBridge();
  unregister = registerScriptBridge(TARGET, bridge);
}

function teardown() {
  unregister?.();
  unregister = null;
  bridge = null;
}

function testContext(overrides = {}) {
  return {
    targetSessionId: TARGET,
    targetLabel: "Test",
    ...overrides,
  };
}

function latestRun() {
  return scriptRuns.value[0];
}

test("stripAnsi removes CSI/OSC/ESC sequences", () => {
  assert.equal(stripAnsi("\x1b[1mhello\x1b[0m world"), "hello world");
  assert.equal(stripAnsi("\x1b]0;title\x07done"), "done");
  assert.equal(stripAnsi("plain"), "plain");
  assert.equal(stripAnsi(""), "");
});

test("send routes text through the script bridge (simulated user input)", async () => {
  setup();
  const run = await runScript(
    {
      id: "s0",
      name: "send-via-bridge",
      code: `await xterm.send("system-view"); await xterm.sendLine("sysname SW1");`,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(sent, ["system-view", "sysname SW1\r"]);
});

test("waitFor resolves on matching string output", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s1",
      name: "wait-string",
      code: `const matched = await xterm.waitFor("[SW1]", 2000); xterm.log("matched", matched);`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "sysname sw1\r\n[SW1]");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "matched [SW1]");
});

test("waitFor matches ansi-wrapped prompts after stripping", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s2",
      name: "wait-ansi",
      code: `await xterm.waitFor("#", 2000);`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "\x1b[1mSW1#\x1b[0m");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
});

test("waitFor consumes matched output so stale prompts do not re-match", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s3",
      name: "wait-consume",
      code: `
        await xterm.waitFor(">", 2000);
        const second = await xterm.waitFor(">", 2000);
        xterm.log("second", second);
      `,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "SW1>");
  await new Promise((resolve) => setTimeout(resolve, 50));
  // 第二次 waitFor 必须等到新的输出，而不是复用已消费的 ">"。
  assert.equal(latestRun().status, SCRIPT_RUN_STATUS.RUNNING);
  publishTerminalOutput(TARGET, "again>");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
});

test("waitFor supports RegExp patterns", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s4",
      name: "wait-regex",
      code: `const m = await xterm.waitFor(/vlan\\s+\\d+/, 2000); xterm.log(m);`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "create vlan 100 done");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "vlan 100");
});

test("terminal output observer errors fail only the script run", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s4b",
      name: "output-observer-error",
      code: `
        class FragileRegExp extends RegExp {
          reads = 0;
          get flags() {
            if (this.reads++ > 0) throw new Error("pattern inspection failed");
            return super.flags;
          }
        }
        await xterm.waitFor(new FragileRegExp("ready"), 2000);
      `,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.doesNotThrow(() => publishTerminalOutput(TARGET, "ready"));
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "pattern inspection failed");
});

test("waitFor timeout fails the run with an i18n default message", async () => {
  setup();
  const run = await runScript(
    {
      id: "s5",
      name: "wait-timeout",
      code: `await xterm.waitFor("never-shows-up", 50);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /超时|timed out/i);
  assert.match(run.error, /never-shows-up/);
});

test("waitFor timeout supports a custom error message", async () => {
  setup();
  const run = await runScript(
    {
      id: "s5b",
      name: "wait-timeout-custom",
      code: `await xterm.waitFor("never", 50, "未等到登录提示符");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "未等到登录提示符");
});

test("an unawaited terminal promise remains managed and reports its rejection", async () => {
  setup();
  const run = await runScript(
    {
      id: "s5c",
      name: "forgotten-await",
      code: `xterm.waitFor("never", 30, "forgotten wait failed");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "forgotten wait failed");
});

test("read collects output during the window", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s6",
      name: "read-window",
      code: `const text = await xterm.read(120); xterm.log(text);`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "line-one\r\n");
  publishTerminalOutput(TARGET, "line-two\r\n");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "line-one\r\nline-two\r\n");
});

test("sleep and log complete the run", async () => {
  setup();
  const run = await runScript(
    {
      id: "s7",
      name: "sleep-log",
      code: `await xterm.sleep(10); xterm.log("done", { ok: true });`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, 'done {"ok":true}');
});

test("standard JavaScript language and Promise features remain available", async () => {
  setup();
  const run = await runScript(
    {
      id: "s7b",
      name: "full-js",
      code: `
        class Counter { constructor(value) { this.value = value; } double() { return this.value * 2; } }
        const values = new Map([["answer", new Counter(21)]]);
        const result = await Promise.resolve(values.get("answer").double());
        console.log({ result });
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, '{"result":42}');
});

test("crt compatibility alias is not exposed", async () => {
  setup();
  const run = await runScript(
    {
      id: "s7c",
      name: "no-crt-alias",
      code: `if (typeof crt !== "undefined") throw new Error("crt alias still exists");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
});

test("stopScript rejects pending waits and marks the run stopped", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s8",
      name: "stop-pending",
      code: `await xterm.waitFor("never", 0);`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(stopScript(latestRun().runId), true);
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.STOPPED);
});

test("stopping a script removes its active prompt and releases the promise", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "s8b",
      name: "stop-prompt",
      code: `await xterm.input("Enter a value");`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(scriptPrompt.value?.message, "Enter a value");
  assert.equal(stopScript(latestRun().runId), true);
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.STOPPED);
  assert.equal(scriptPrompt.value, null);
});

test("stale dialog events cannot resolve the next queued script prompt", async () => {
  const first = requestScriptPrompt({ kind: "input", runId: "prompt-a" });
  const firstRequestId = scriptPrompt.value.requestId;
  const second = requestScriptPrompt({ kind: "input", runId: "prompt-b" });

  assert.equal(resolveScriptPrompt("first", firstRequestId), true);
  assert.equal(await first, "first");
  const secondRequestId = scriptPrompt.value.requestId;
  assert.notEqual(secondRequestId, firstRequestId);
  assert.equal(resolveScriptPrompt("stale", firstRequestId), false);
  assert.equal(scriptPrompt.value.requestId, secondRequestId);
  assert.equal(resolveScriptPrompt("second", secondRequestId), true);
  assert.equal(await second, "second");
  assert.equal(scriptPrompt.value, null);
});

test("run fails fast when the target session has no script bridge", async () => {
  const run = await runScript(
    {
      id: "s9",
      name: "no-bridge",
      code: `await xterm.sendLine("system-view");`,
    },
    testContext(),
  );
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /not available/);
});

test("syntax errors fail before script execution", async () => {
  setup();
  const run = await runScript(
    { id: "syntax-error", name: "syntax-error", code: "if (" },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /syntax|unexpected|token/i);
});

test("runScript rejects runs without a target session", async () => {
  const run = await runScript(
    { id: "s10", name: "no-target", code: "" },
    testContext({ targetSessionId: "" }),
  );
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
});

test("errors thrown by asynchronous timer callbacks are captured", async () => {
  setup();
  const run = await runScript(
    {
      id: "s11",
      name: "timer-error",
      code: `setTimeout(() => { throw new Error("timer exploded"); }, 5);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "timer exploded");
});

test("microtask errors are captured by the script runtime", async () => {
  setup();
  const run = await runScript(
    {
      id: "s11b",
      name: "microtask-error",
      code: `queueMicrotask(() => { throw new Error("microtask exploded"); });`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "microtask exploded");
});

test("cleared intervals release the event-driven runtime without polling", async () => {
  setup();
  const run = await runScript(
    {
      id: "s11c",
      name: "interval-cleanup",
      code: `const timer = setInterval(() => clearInterval(timer), 5);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
});

test("runtime setup errors are returned as failed runs instead of escaping", async () => {
  const brokenBridge = createFakeBridge();
  brokenBridge.onOutput = () => {
    throw new Error("bridge subscription failed");
  };
  const unregisterBroken = registerScriptBridge(TARGET, brokenBridge);
  const run = await runScript(
    { id: "s12", name: "setup-error", code: `console.log("never runs");` },
    testContext(),
  );
  unregisterBroken();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "bridge subscription failed");
});

test("getBuffer returns the existing terminal buffer text", async () => {
  setup();
  bridge.bufferText = "line one\nline two\nline three";
  const run = await runScript(
    {
      id: "s13",
      name: "get-buffer",
      code: `xterm.log(xterm.getBuffer());`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "line one\nline two\nline three");
});

test("searchBuffer finds substring and regex matches with line numbers", async () => {
  setup();
  bridge.bufferText = "alpha\nerror: first\nbeta\nerror: second";
  const run = await runScript(
    {
      id: "s14",
      name: "search-buffer",
      code: `
        const byText = xterm.searchBuffer("error");
        xterm.log("text", byText.count, byText.matches[0].line, byText.matches[1].line);
        const byRegex = xterm.searchBuffer(/^error: second$/);
        xterm.log("regex", byRegex.count, byRegex.matches[0].text);
        const none = xterm.searchBuffer("missing");
        xterm.log("none", none.count, none.matches.length);
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "text 2 2 4");
  assert.equal(run.logs[1].text, "regex 1 error: second");
  assert.equal(run.logs[2].text, "none 0 0");
});

test("searchBuffer rejects invalid pattern types", async () => {
  setup();
  const run = await runScript(
    {
      id: "s14b",
      name: "search-buffer-invalid",
      code: `xterm.searchBuffer(42);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /string or RegExp/);
});

test("startRecording/stopRecording drive the session recording bridge", async () => {
  setup();
  const recording = createFakeRecording();
  const unregisterRecording = registerRecordingBridge(recording);
  const run = await runScript(
    {
      id: "s15",
      name: "recording",
      code: `
        const path = await xterm.startRecording();
        xterm.log("started", path, xterm.isRecording());
        const stopped = await xterm.stopRecording();
        xterm.log("stopped", stopped, xterm.isRecording());
      `,
    },
    testContext(),
  );
  unregisterRecording();
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "started /tmp/recording.txt true");
  assert.equal(run.logs[1].text, "stopped /tmp/recording.txt false");
});

test("canceling the recording save dialog stops the script run", async () => {
  setup();
  const recording = createFakeRecording("");
  const unregisterRecording = registerRecordingBridge(recording);
  const run = await runScript(
    {
      id: "s15b",
      name: "recording-cancel",
      code: `await xterm.startRecording(); xterm.log("never reached");`,
    },
    testContext(),
  );
  unregisterRecording();
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.STOPPED);
  assert.equal(run.logs.length, 0);
});

test("recording api fails fast when no recording controller is registered", async () => {
  setup();
  const run = await runScript(
    {
      id: "s15c",
      name: "recording-unavailable",
      code: `await xterm.startRecording();`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /recording is not available/);
});
