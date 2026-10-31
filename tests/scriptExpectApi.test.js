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

test("waitForAny preserves per-pattern RegExp flags", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e8b",
      name: "wait-any-flags",
      code: `
        const hit = await xterm.waitForAny(["prompt>", /error:\\s*(\\w+)/i], 2000);
        xterm.log(hit);
      `,
    },
    testContext(),
  );
  await settle();
  // 仅小写 "error" 模式带 i flag：合并正则丢 flags 时这里会超时而非命中。
  publishTerminalOutput(TARGET, "ERROR: Timeout");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "ERROR: Timeout");
});

test("waitForAny picks the earliest match among mixed string/RegExp patterns", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e8c",
      name: "wait-any-order",
      code: `const hit = await xterm.waitForAny(["beta", /a/], 2000); xterm.log(hit);`,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, "xxbeta-alpha");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.equal(run.logs[0].text, "beta");
});

test("waitForAny rejects an empty or invalid pattern list", async () => {
  setup();
  const run = await runScript(
    {
      id: "e8d",
      name: "wait-any-invalid",
      code: `await xterm.waitForAny(["ok", 42]);`,
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

test("press backspace follows the session backspace mode", async () => {
  setup();
  const run = await runScript(
    {
      id: "e9b",
      name: "press-backspace",
      code: `await xterm.press("backspace");`,
    },
    testContext({ sessionInfo: { backspaceSends: "BS" } }),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(sent, ["\x08"]);
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

test("expect ring-buffer truncation does not expose an orphaned low surrogate", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e13",
      name: "expect-grapheme-suffix",
      code: `
        const hit = await xterm.expect("END", 2000);
        xterm.log(JSON.stringify({
          length: hit.before.length,
          wellFormed: hit.before.isWellFormed(),
          content: hit.before === "b".repeat(65535),
        }));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, `${"a".repeat(65536)}🙂${"b".repeat(65535)}`);
  publishTerminalOutput(TARGET, "END");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(JSON.parse(run.logs[0].text), {
    length: 65535,
    wellFormed: true,
    content: true,
  });
});

test("expect ring-buffer truncation retains an emoji starting at the cut point", async () => {
  setup();
  const runPromise = runScript(
    {
      id: "e14",
      name: "expect-grapheme-suffix-boundary",
      code: `
        const hit = await xterm.expect("END", 2000);
        xterm.log(JSON.stringify({
          length: hit.before.length,
          wellFormed: hit.before.isWellFormed(),
          content: hit.before === "🙂" + "b".repeat(65534),
        }));
      `,
    },
    testContext(),
  );
  await settle();
  publishTerminalOutput(TARGET, `${"a".repeat(65537)}🙂${"b".repeat(65534)}`);
  publishTerminalOutput(TARGET, "END");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE);
  assert.deepEqual(JSON.parse(run.logs[0].text), {
    length: 65536,
    wellFormed: true,
    content: true,
  });
});

test("expectAny retries timeouts and sends the configured retry input", async () => {
  setup();
  const send = bridge.send.bind(bridge);
  bridge.send = (data) => {
    const result = send(data);
    if (data === "\r") {
      const attempt = bridge.sent.filter((value) => value === "\r").length;
      if (attempt === 3) setTimeout(() => publishTerminalOutput(TARGET, "SW1#"), 0);
    }
    return result;
  };
  const run = await runScript(
    {
      id: "e15",
      name: "expect-retry",
      code: `
        const hit = await xterm.expectAny([/#/], {
          timeout: 10,
          retryDelays: [1, 2, 3],
          message: "retry failed",
        });
        xterm.log(hit.text, hit.index);
      `,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE, run.error);
  assert.equal(sent.filter((value) => value === "\r").length, 3);
  assert.equal(run.logs[0].text, "# 0");
});

test("expect retry exhaustion uses the custom error message", async () => {
  setup();
  const run = await runScript(
    {
      id: "e16",
      name: "expect-retry-exhausted",
      code: `
        await xterm.expect(/never/, {
          timeout: 5,
          retryDelays: [1, 2],
          message: "no prompt after retries",
        });
      `,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "no prompt after retries");
  assert.equal(sent.filter((value) => value === "\r").length, 2);
});

test("sendAndExpect discards stale output before sending the command", async () => {
  setup();
  const send = bridge.send.bind(bridge);
  bridge.send = (data) => {
    const result = send(data);
    if (data === "show version\r") {
      setTimeout(() => publishTerminalOutput(TARGET, "show version\r\nfresh#"), 0);
    }
    return result;
  };
  const runPromise = runScript(
    {
      id: "e17",
      name: "send-and-expect",
      code: `
        await xterm.sleep(20);
        const hit = await xterm.sendAndExpect("show version", /#/, 1000);
        xterm.log(hit.before);
      `,
    },
    testContext(),
  );
  await settle(5);
  publishTerminalOutput(TARGET, "stale#");
  const run = await runPromise;
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE, run.error);
  assert.equal(run.logs[0].text, "show version\r\nfresh");
});

test("runCommand handles pagination and returns the combined output", async () => {
  setup();
  const send = bridge.send.bind(bridge);
  let page = 0;
  bridge.send = (data) => {
    const result = send(data);
    if (data === "dir\r") {
      setTimeout(() => publishTerminalOutput(TARGET, "dir\r\nfirst\r\n-- More --"), 0);
    } else if (data === " ") {
      page++;
      setTimeout(() => publishTerminalOutput(TARGET, `\r\npage-${page}\r\nSW1#`), 0);
    }
    return result;
  };
  const run = await runScript(
    {
      id: "e18",
      name: "run-command-pages",
      code: `
        const result = await xterm.runCommand("dir", {
          prompt: /#/,
          more: /More/,
          timeout: 100,
          maxPages: 2,
        });
        xterm.log(JSON.stringify(result));
      `,
    },
    testContext(),
  );
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.DONE, run.error);
  const result = JSON.parse(run.logs[0].text);
  assert.equal(result.pages, 1);
  assert.equal(result.text, "#");
  assert.match(result.output, /first/);
  assert.match(result.output, /page-1/);
});

test("runCommand stops at maxPages instead of paging forever", async () => {
  setup();
  const send = bridge.send.bind(bridge);
  bridge.send = (data) => {
    const result = send(data);
    if (data === "dir\r") {
      setTimeout(() => publishTerminalOutput(TARGET, "dir\r\n-- More --"), 0);
    }
    return result;
  };
  const run = await runScript(
    {
      id: "e19",
      name: "run-command-page-limit",
      code: `
        await xterm.runCommand("dir", {
          prompt: /#/,
          more: /More/,
          timeout: 100,
          maxPages: 0,
        });
      `,
    },
    testContext(),
  );
  const sent = [...bridge.sent];
  teardown();
  assert.equal(run.status, SCRIPT_RUN_STATUS.ERROR);
  assert.equal(run.error, "dir exceeded maxPages (0)");
  assert.equal(sent.includes(" "), false);
});
