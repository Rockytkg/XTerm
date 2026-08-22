import assert from "node:assert/strict";
import test from "node:test";
import { CONNECTION_EVENT, reduceConnectionState } from "../src/stores/connectionStateMachine.js";

test("serial baud detection blocks only for the duration of redetection", () => {
  const detecting = reduceConnectionState(
    { status: "connected", phase: null, error: null },
    { type: CONNECTION_EVENT.SERIAL_REDETECT_REQUESTED },
  );

  assert.equal(detecting.status, "connected");
  assert.equal(detecting.phase, "serialBaudDetection");

  const connected = reduceConnectionState(detecting, {
    type: CONNECTION_EVENT.SERIAL_REDETECT_SUCCEEDED,
    payload: { baudRate: 115200, confirmed: true, serialPort: "COM3", serialScores: [] },
  });

  assert.equal(connected.status, "connected");
  assert.equal(connected.phase, null);
  assert.equal(connected.detectedBaudRate, 115200);
  assert.equal(connected.detectedBaudConfirmed, true);
});

test("a Telnet negotiation failure is not downgraded by a later close event", () => {
  const failed = reduceConnectionState(
    { status: "connected", phase: null, error: null },
    {
      type: CONNECTION_EVENT.SESSION_FAILED,
      payload: { detail: "Telnet negotiation failed" },
    },
  );
  const closed = reduceConnectionState(failed, {
    type: CONNECTION_EVENT.SESSION_CLOSED,
    payload: { detail: "Telnet connection closed by remote host" },
  });

  assert.equal(closed.status, "failed");
  assert.equal(closed.statusDetail, "Telnet negotiation failed");
});

test("a final lifecycle state ignores every stale terminal event until reconnect", () => {
  const failed = reduceConnectionState(
    { status: "connecting", phase: "connecting", error: null },
    {
      type: CONNECTION_EVENT.SESSION_FAILED,
      payload: { detail: "SSH transport write failed" },
    },
  );

  for (const event of [
    { type: CONNECTION_EVENT.SESSION_READY },
    { type: CONNECTION_EVENT.SESSION_CLOSED, payload: { detail: "late close" } },
    {
      type: CONNECTION_EVENT.OPEN_FAILED,
      payload: { error: { code: "late_open_error", detail: "late open error" } },
    },
  ]) {
    assert.strictEqual(reduceConnectionState(failed, event), failed);
  }

  const reconnecting = reduceConnectionState(failed, { type: CONNECTION_EVENT.OPEN_REQUESTED });
  const connected = reduceConnectionState(reconnecting, {
    type: CONNECTION_EVENT.SESSION_READY,
  });
  assert.equal(connected.status, "connected");
  assert.equal(connected.error, null);
});

test("a normal close is not upgraded to failure by a stale worker event", () => {
  const closed = reduceConnectionState(
    { status: "connected", phase: null, error: null },
    { type: CONNECTION_EVENT.SESSION_CLOSED, payload: { detail: "remote closed" } },
  );
  const staleFailure = reduceConnectionState(closed, {
    type: CONNECTION_EVENT.SESSION_FAILED,
    payload: { detail: "event channel disconnected" },
  });

  assert.strictEqual(staleFailure, closed);
  assert.equal(staleFailure.status, "closed");
});
