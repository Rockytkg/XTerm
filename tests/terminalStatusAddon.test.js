import assert from "node:assert/strict";
import test from "node:test";

import unicode11Package from "@xterm/addon-unicode11";
import xtermPackage from "@xterm/xterm";

import { TerminalStatusAddon } from "../src/utils/terminal/addons/status/TerminalStatusAddon.js";

const { Unicode11Addon } = unicode11Package;
const { Terminal } = xtermPackage;

function visibleLines(terminal) {
  return Array.from({ length: terminal.buffer.active.length }, (_, index) =>
    terminal.buffer.active.getLine(index)?.translateToString(true),
  ).filter(Boolean);
}

function createRealStatusAddon(t, { cols = 12, rows = 16, host = "example.com" } = {}) {
  const terminal = new Terminal({ allowProposedApi: true, cols, rows });
  terminal.loadAddon(new Unicode11Addon());
  terminal.unicode.activeVersion = "11";

  const translations = {
    "terminal.connectionConnected": "Connected",
    "terminal.connectionConnectingSsh": "Connecting",
    "terminal.connectionFailed": "Failed",
  };
  const pendingWrites = [];
  const writes = [];
  const addon = new TerminalStatusAddon({
    getConnection: () => ({ host, port: 22, protocol: "ssh" }),
    getDetectedBaudRate: () => 0,
    getFailureDetail: () => "connection refused",
    getFailureLabel: () => "Network error",
    getPalette: () => ({
      boot: "#000002",
      error: "#000001",
      hint: "#000003",
      success: "#000004",
    }),
    getStatusDetail: () => "closed",
    queueWrite: (data) => {
      writes.push(data);
      pendingWrites.push(new Promise((resolve) => terminal.write(data, resolve)));
    },
    t: (key) => translations[key] || key,
  });
  addon.activate(terminal);
  t.after(async () => {
    await Promise.all(pendingWrites);
    addon.dispose();
    terminal.dispose();
  });

  return {
    addon,
    terminal,
    translations,
    writes,
    flush: () => Promise.all(pendingWrites),
  };
}

function createStatusAddon(cursorX = 0, protocol = "ssh") {
  const writes = [];
  const addon = new TerminalStatusAddon({
    getConnection: () => ({ host: "example.com", port: 22, protocol }),
    getFailureDetail: () => "connection refused",
    getFailureLabel: () => "Network error",
    getPalette: () => ({
      boot: "#000002",
      error: "#000001",
      hint: "#000003",
      success: "#000004",
    }),
    getStatusDetail: () => "closed",
    queueWrite: (data) => writes.push(data),
    t: (key) => key,
  });
  addon.activate({ buffer: { active: { cursorX } } });
  return { addon, writes };
}

test("failure status always starts with a blank line before the message", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("failed", "connection refused");

  assert.equal(writes.length, 1);
  assert.equal(writes[0].startsWith("\r\n"), true);
});

test("failure status moves to a new line when output ends mid-line", () => {
  const { addon, writes } = createStatusAddon(5);

  addon.write("failed", "connection refused");

  assert.equal(writes.length, 1);
  assert.equal(writes[0].startsWith("\r\n"), true);
});

test("reconnect failure replaces the progress block without a leading blank line", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("connecting");
  addon.write("failed", "connection refused");

  assert.equal(writes.length, 2);
  assert.equal(writes[1].startsWith(`\r${"\x1b[1A\x1b[2K".repeat(2)}`), true);
  assert.equal(writes[1].startsWith("\r\n"), false);
});

test("connection success replaces the complete progress block", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("connecting");
  addon.write("connected");

  assert.equal(writes.length, 2);
  assert.equal(writes[1].startsWith(`\r${"\x1b[1A\x1b[2K".repeat(2)}`), true);
  assert.equal(writes[1].includes("terminal.connectionConnected"), true);
  assert.equal(writes[1].includes("example.com"), false);
});

test("released connection progress can be shown by a later attempt", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("connecting");
  addon.release();
  addon.write("connecting");

  assert.equal(writes.length, 3);
  assert.equal(writes[1], `\r${"\x1b[1A\x1b[2K".repeat(2)}`);
  assert.equal(writes[2].includes("terminal.connectionConnectingSsh"), true);
});

test("connected serial auto-baud status shows the detected baud rate", () => {
  const writes = [];
  const addon = new TerminalStatusAddon({
    getConnection: () => ({ protocol: "serial", baudRate: "auto" }),
    getDetectedBaudRate: () => 115200,
    getFailureDetail: () => "",
    getFailureLabel: () => "",
    getPalette: () => ({ success: "#00ff00", hint: "#ffffff" }),
    getStatusDetail: () => "",
    queueWrite: (data) => writes.push(data),
    t: (key, args = {}) =>
      key === "terminal.serialAutoBaudDetected" ? `using ${args.baudRate}` : key,
  });
  addon.activate({});

  addon.write("connected");

  assert.match(writes[0], /using 115200/);
});

test("an early Telnet negotiation failure replaces the tracked connection progress", () => {
  const { addon, writes } = createStatusAddon(0, "telnet");

  addon.write("connecting");
  addon.write(
    "failed",
    "Telnet negotiation failed: remote host closed the connection before the session became ready",
  );

  const eraseProgress = `\r${"\x1b[1A\x1b[2K".repeat(2)}`;
  assert.equal(writes.length, 2);
  assert.equal(writes[1].startsWith(eraseProgress), true);
  assert.equal(writes[1].includes("terminal.connectionFailed"), true);
  assert.equal(writes[1].includes("example.com"), false);
});

test("discarded Telnet reconnect progress can be replayed after a status reset", () => {
  const { addon, writes } = createStatusAddon(0, "telnet");

  addon.write("connecting");
  writes.length = 0;
  addon.reset();
  addon.write("connecting");

  assert.equal(writes.length, 1);
  assert.equal(writes[0].includes("terminal.connectionConnectingTelnet"), true);
});

test("an unchanged reactive failure replay is idempotent", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("failed", "connection refused");
  addon.write("failed");

  assert.equal(writes.length, 1);
});

test("a connected banner released by device output stays idempotent", () => {
  // 后台挂起只丢弃未落屏的输出队列，屏幕与横幅都保留；此后由状态重放
  // （如串口波特率重检测）触发的同内容 write 必须仍是空操作，否则横幅
  // 会被重复追加到当前光标处。
  const { addon, writes } = createStatusAddon(0);

  addon.write("connected");
  addon.release();
  addon.write("connected");

  assert.equal(writes.length, 1);
});

test("a localized connection error is followed by the raw backend detail on a separate line", () => {
  for (const protocol of ["ssh", "telnet", "serial"]) {
    const { addon, writes } = createStatusAddon(0, protocol);

    addon.write("failed", "port=auto; no serial ports were found");

    assert.equal(writes.length, 1);
    assert.equal(writes[0].includes("Network error"), true);
    assert.equal(writes[0].includes("Network error\x1b[0m\r\n"), true);
    assert.equal(writes[0].includes("port=auto; no serial ports were found"), true);
  }
});

test("a failure detail refresh replaces the current lifecycle block", () => {
  const writes = [];
  let failureLabel = "";
  const addon = new TerminalStatusAddon({
    getConnection: () => ({ host: "example.com", port: 23, protocol: "telnet" }),
    getFailureDetail: () => "Telnet negotiation failed",
    getFailureLabel: () => failureLabel,
    getPalette: () => ({
      boot: "#000002",
      error: "#000001",
      hint: "#000003",
      success: "#000004",
    }),
    getStatusDetail: () => "closed",
    queueWrite: (data) => writes.push(data),
    t: (key) => key,
  });
  addon.activate({ buffer: { active: { cursorX: 0 } } });

  addon.write("failed", "Telnet negotiation failed");
  failureLabel = "The terminal session failed to start";
  addon.write("failed", "Telnet negotiation failed");

  assert.equal(writes.length, 2);
  assert.equal(writes[1].startsWith(`\r${"\x1b[1A\x1b[2K".repeat(2)}`), true);
  assert.equal(writes[1].includes("The terminal session failed to start"), true);
  assert.equal(writes[1].includes("Telnet negotiation failed"), true);
});

test("narrow columns fully replace a soft-wrapped ASCII progress block", async (t) => {
  const { addon, terminal, flush } = createRealStatusAddon(t, {
    host: "ascii-host-with-a-long-name.example.com",
  });

  addon.write("connecting");
  await flush();
  assert.ok(visibleLines(terminal).length > 2);

  addon.write("connected");
  await flush();

  assert.deepEqual(visibleLines(terminal), ["Connected"]);
});

test("release erases every soft-wrapped CJK progress row", async (t) => {
  const { addon, terminal, flush } = createRealStatusAddon(t, {
    host: "数据中心节点-这是一个需要多行显示的终端主机名",
  });

  addon.write("connecting");
  await flush();
  assert.ok(visibleLines(terminal).length > 2);

  addon.release();
  await flush();

  assert.deepEqual(visibleLines(terminal), []);
});

test("a changed lifecycle status replaces all soft-wrapped rows", async (t) => {
  const { addon, terminal, flush } = createRealStatusAddon(t, { cols: 20 });

  addon.write("failed", "first-detail-that-wraps-over-several-terminal-rows");
  await flush();
  addon.write("failed", "replacement");
  await flush();

  assert.deepEqual(visibleLines(terminal), ["Failed", "Network error", "replacement"]);
});

test("resize recomputes saved progress rows before connected replacement", async (t) => {
  const { addon, terminal, flush } = createRealStatusAddon(t, {
    cols: 60,
    host: "resize-host-with-a-long-name.example.com",
  });

  addon.write("connecting");
  await flush();
  assert.deepEqual(visibleLines(terminal), [
    "Connecting",
    "SSH resize-host-with-a-long-name.example.com:22",
  ]);

  terminal.resize(10, 16);
  assert.ok(visibleLines(terminal).length > 2);
  addon.write("connected");
  await flush();

  assert.deepEqual(visibleLines(terminal), ["Connected"]);
});

test("a banner starting away from column zero counts its first row offset", async (t) => {
  const { addon, terminal, translations, flush } = createRealStatusAddon(t, { cols: 10 });
  await new Promise((resolve) => terminal.write("12345", resolve));

  addon.write("connected");
  await flush();
  assert.ok(visibleLines(terminal).length > 1);

  translations["terminal.connectionConnected"] = "OK";
  addon.write("connected");
  await flush();

  assert.deepEqual(visibleLines(terminal), ["OK"]);
});

test("embedded CR and LF become separately counted logical lines", async (t) => {
  const { addon, terminal, flush } = createRealStatusAddon(t, { cols: 40 });

  addon.write("failed", "alpha\r\nbravo\rcharlie\ndelta");
  await flush();
  assert.deepEqual(visibleLines(terminal), [
    "Failed",
    "Network error",
    "alpha",
    "bravo",
    "charlie",
    "delta",
  ]);

  addon.write("failed", "done");
  await flush();

  assert.deepEqual(visibleLines(terminal), ["Failed", "Network error", "done"]);
});

test("dispose unregisters the terminal resize listener", () => {
  const { addon } = createStatusAddon();
  let disposeCount = 0;
  addon.activate({
    buffer: { active: { cursorX: 0 } },
    onResize: () => ({ dispose: () => disposeCount++ }),
  });

  addon.dispose();

  assert.equal(disposeCount, 1);
});
