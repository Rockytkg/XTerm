import assert from "node:assert/strict";
import test from "node:test";
import { createScriptExecutorHost } from "../src/services/scripting/scriptExecutorHost.js";

function withWorker(workerClass, callback) {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", {
    configurable: true,
    writable: true,
    value: workerClass,
  });
  try {
    return callback();
  } finally {
    if (descriptor) Object.defineProperty(globalThis, "Worker", descriptor);
    else delete globalThis.Worker;
  }
}

function hostOptions() {
  return {
    code: `xterm.log("executed");`,
    api: { log() {}, session: {} },
    run: { aborted: false },
    log() {},
    hostRuntime: { fail() {} },
    formatBlockedMessage: (name) => name,
  };
}

test("Worker construction failure does not fall back to main-thread script execution", () => {
  withWorker(
    class {
      constructor() {
        throw new Error("worker creation denied");
      }
    },
    () => {
      assert.throws(() => createScriptExecutorHost(hostOptions()), /worker creation denied/);
    },
  );
});

test("Worker startup message failure terminates the partially created Worker", () => {
  let terminated = false;
  withWorker(
    class {
      constructor() {}
      postMessage() {
        throw new Error("message clone failed");
      }
      terminate() {
        terminated = true;
      }
    },
    () => {
      assert.throws(() => createScriptExecutorHost(hostOptions()), /message clone failed/);
    },
  );
  assert.equal(terminated, true);
});
