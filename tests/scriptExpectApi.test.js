import assert from "node:assert/strict";
import test from "node:test";
import {
  SCRIPT_RUN_STATUS,
  runScript,
  scriptRuns,
} from "../src/services/scripting/scriptRunner.js";
import { publishTerminalOutput, registerScriptBridge } from "../src/services/scripting/bridges.js";

const TARGET = "expect-test-session";

// 模拟 ScriptBridgeAddon 的测试桩：记录 send 的文本，提供输出订阅。
function createFakeBridge() {
  const listeners = new Set();
  return {
    sent: [],
    send(data) {
      this.sent.push(data);
      return true;
    },
    getScreenText: () => "",
    getBufferText: () => "",
    notifyOutput(data) {
      for (const listener of [...listeners]) listener(data);
    },
    onOutput(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
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

async function settle(ms = 20) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

test("expect returns matched text, capture groups and preceding output", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e1",
      name: "expect-groups",
      code: `
        const hit = await xterm.expect(/uptime is (\\d+) days/, 2000);
        xterm.log(JSON.stringify({ text: hit.text, days: hit.groups[0], before: hit.before }));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, "display version\r\nSystem uptime is 25 days, 3 hours");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  const logged = JSON.parse(run.logs[0].text);
  assert.equal(logged.text, "uptime is 25 days");
  assert.equal(logged.days, "25");
  assert.equal(logged.before, "display version\r\nSystem ");
});

test("expect consumes the match so the next expect only sees fresh output", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e2",
      name: "expect-consume",
      code: `
        const first = await xterm.expect(">", 2000);
        const second = await xterm.expect(">", 2000);
        xterm.log(JSON.stringify({ first: first.before, second: second.before }));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, "SW1>");
  await settle(50);
  assert.equal(latestRun().status, SCRIPT_RUN_STATUS.RUNNING);
  publishTerminalOutput(TARGET, "vlan 100\r\n[SW1]>");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  const logged = JSON.parse(run.logs[0].text);
  assert.equal(logged.first, "SW1");
  assert.equal(logged.second, "vlan 100\r\n[SW1]");
});

test("expect with a string pattern returns empty groups", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e3",
      name: "expect-string",
      code: `
        const hit = await xterm.expect("[SW1]", 2000);
        xterm.log(hit.text, hit.groups.length, JSON.stringify(hit.before));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, "system-view\r\n[SW1]");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, '[SW1] 0 "system-view\\r\\n"');
});

test("expect timeout fails the run with the shared wait-timeout message", async () => {
  setup();
  const run = await runScript(
    {
      id: "e4",
      name: "expect-timeout",
      code: `await xterm.expect(/never/, 50, "未等到版本信息");`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "未等到版本信息");
});

test("expect rejects invalid pattern types", async () => {
  setup();
  const run = await runScript(
    {
      id: "e5",
      name: "expect-invalid",
      code: `await xterm.expect(42);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /string or RegExp/);
});

test("expectAny reports which pattern matched with its groups", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e6",
      name: "expect-any-index",
      code: `
        const hit = await xterm.expectAny(["[SW1]", /(Error|Failed): (.*)/], 2000);
        xterm.log(JSON.stringify({ index: hit.index, text: hit.text, reason: hit.groups[1] }));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, "sysname\r\nError: duplicate hostname");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  const logged = JSON.parse(run.logs[0].text);
  assert.equal(logged.index, 1);
  assert.equal(logged.text, "Error: duplicate hostname");
  assert.equal(logged.reason, "duplicate hostname");
});

test("expectAny picks the earliest match and prefers the first pattern on ties", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e7",
      name: "expect-any-order",
      code: `
        const hit = await xterm.expectAny(["beta", "alpha", /a/], 2000);
        xterm.log(hit.index, hit.text);
      `,
    },
    testContext(),
  );
  await settle();
  // "beta" 与 "a" 同起于 index 2，靠前的 "beta" 优先；"alpha" 位于 index 8 更靠后。
  publishTerminalOutput(TARGET, "xxbeta-alpha");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "0 beta");
});

test("expectAny rejects an empty or invalid pattern list", async () => {
  setup();
  const run = await runScript(
    {
      id: "e8",
      name: "expect-any-invalid",
      code: `await xterm.expectAny(["ok", 42]);`,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /strings or RegExps/);
});

test("press maps named keys and ctrl combinations to control sequences", async () => {
  setup();
  const run = await runScript(
    {
      id: "e9",
      name: "press-keys",
      code: `
        await xterm.press("ctrl+c");
        await xterm.press("ENTER");
        await xterm.press("esc");
        await xterm.press("up");
        await xterm.press("f5");
        await xterm.press("ctrl-Z");
      `,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(sent, ["\x03", "\r", "\x1b", "\x1b[A", "\x1b[15~", "\x1a"]);
});

test("press rejects unknown key names without sending anything", async () => {
  setup();
  const run = await runScript(
    {
      id: "e10",
      name: "press-unknown",
      code: `await xterm.press("win+shift+s");`,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.match(run.error, /ctrl\+<letter>|named key/);
  assert.equal(sent.length, 0);
});

test("session exposes non-sensitive connection metadata from the run context", async () => {
  setup();
  const run = await runScript(
    {
      id: "e11",
      name: "session-meta",
      code: `
        const s = xterm.session;
        xterm.log([s.id, s.label, s.protocol, s.host, s.port, s.username].join("|"));
      `,
    },
    testContext({
      sessionInfo: { protocol: "ssh", host: "10.0.0.1", port: 22, username: "admin" },
    }),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, `${TARGET}|Test|ssh|10.0.0.1|22|admin`);
});

test("session metadata defaults to empty values when the context omits it", async () => {
  setup();
  const run = await runScript(
    {
      id: "e12",
      name: "session-meta-default",
      code: `
        const s = xterm.session;
        xterm.log(JSON.stringify([s.protocol, s.host, s.port, s.username]));
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, '["","","",""]');
});
