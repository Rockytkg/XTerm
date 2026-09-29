import test from "node:test";
import assert from "node:assert/strict";
import {
  findReusableSerialSession,
  serialConnectionProfileKey,
} from "../src/utils/serialConnectionProfile.js";

const profile = {
  protocol: "serial",
  host: "COM4",
  serialPort: "COM4",
  baudRate: "auto",
  serialQuickAutoBaud: true,
  dataBits: 8,
  flowControl: "none",
  parity: "none",
  stopBits: 1,
  encoding: "utf-8",
};

test("serial profile key tracks link settings but ignores presentation-only edits", () => {
  assert.equal(
    serialConnectionProfileKey({ ...profile, name: "Renamed connection" }),
    serialConnectionProfileKey(profile),
  );
  assert.notEqual(
    serialConnectionProfileKey({ ...profile, baudRate: 115200 }),
    serialConnectionProfileKey(profile),
  );
});

test("a serial frontend session opened with the current profile can be reused or retried", () => {
  const session = { id: "terminal-1", sessionId: "backend-1" };
  const sessions = [session];
  const profileKeys = new Map([[session.id, serialConnectionProfileKey(profile)]]);

  assert.equal(findReusableSerialSession(sessions, profileKeys, profile), session);
  assert.equal(
    findReusableSerialSession(sessions, profileKeys, { ...profile, parity: "even" }),
    undefined,
  );
  const failedSession = { ...session, sessionId: "" };
  assert.equal(findReusableSerialSession([failedSession], profileKeys, profile), failedSession);
  assert.equal(
    findReusableSerialSession([failedSession], profileKeys, { ...profile, parity: "even" }),
    undefined,
  );
});
