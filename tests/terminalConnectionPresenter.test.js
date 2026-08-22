import assert from "node:assert/strict";
import test from "node:test";
import { TerminalConnectionPresenter } from "../src/utils/terminal/TerminalConnectionPresenter.js";

test("reset invalidates discarded output before replaying the current status", () => {
  const calls = [];
  const presenter = new TerminalConnectionPresenter({
    dropOutput: () => calls.push("drop"),
    getState: () => ({ status: "connecting", phase: "connecting" }),
    handleStatus: (status, phase) => calls.push(`${status}:${phase}`),
    onConnecting: () => calls.push("presented"),
    resetStatus: () => calls.push("reset"),
  });

  presenter.reset({ replay: true });

  assert.deepEqual(calls, ["drop", "reset", "connecting:connecting", "presented"]);
});

test("an ordinary reset does not invent an optimistic connection state", () => {
  const statuses = [];
  const presenter = new TerminalConnectionPresenter({
    dropOutput() {},
    getState: () => ({ status: "failed" }),
    handleStatus: (status) => statuses.push(status),
    resetStatus() {},
  });

  presenter.reset();

  assert.deepEqual(statuses, []);
});

test("a preserved backend session transition cannot discard a queued failure presentation", () => {
  const calls = [];
  const presenter = new TerminalConnectionPresenter({
    dropOutput: () => calls.push("drop"),
    getState: () => ({ status: "failed", phase: null }),
    handleStatus: (status) => calls.push(status),
    resetStatus: () => calls.push("reset"),
  });

  presenter.resetBackendSession({ preserveViewport: true });

  assert.deepEqual(calls, []);
});

test("a replaced backend viewport resets and replays the current connection state", () => {
  const calls = [];
  const presenter = new TerminalConnectionPresenter({
    dropOutput: () => calls.push("drop"),
    getState: () => ({ status: "connecting", phase: "connecting" }),
    handleStatus: (status) => calls.push(status),
    resetStatus: () => calls.push("reset"),
  });

  presenter.resetBackendSession();

  assert.deepEqual(calls, ["drop", "reset", "connecting"]);
});
