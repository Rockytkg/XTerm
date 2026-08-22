import assert from "node:assert/strict";
import test from "node:test";
import { TerminalStatusAddon } from "../src/utils/terminal/addons/status/TerminalStatusAddon.js";

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

test("failure status does not add a blank line when already at line start", () => {
  const { addon, writes } = createStatusAddon(0);

  addon.write("failed", "connection refused");

  assert.equal(writes.length, 1);
  assert.equal(writes[0].startsWith("\r\n"), false);
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
