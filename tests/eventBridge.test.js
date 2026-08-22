import assert from "node:assert/strict";
import test from "node:test";
import { createEventBridge } from "../src/utils/eventBridge.js";

function createLoggerStub() {
  return { error() {} };
}

test("event bridge observes events emitted while the native listener starts", async () => {
  const payloads = [];
  const observe = createEventBridge({
    eventName: "state-change",
    logName: "state.change",
    logger: createLoggerStub(),
    subscribe(_eventName, handler) {
      handler({ payload: { revision: 1 } });
      return () => {};
    },
  });

  const dispose = await observe((payload) => payloads.push(payload));
  assert.deepEqual(payloads, [{ revision: 1 }]);
  dispose();
});

test("event bridge keeps duplicate handlers isolated by subscription", async () => {
  let emit;
  let startCount = 0;
  let stopCount = 0;
  let callCount = 0;
  const handler = () => {
    callCount += 1;
  };
  const observe = createEventBridge({
    eventName: "state-change",
    logName: "state.change",
    logger: createLoggerStub(),
    subscribe(_eventName, listener) {
      startCount += 1;
      emit = listener;
      return () => {
        stopCount += 1;
      };
    },
  });

  const [disposeFirst, disposeSecond] = await Promise.all([observe(handler), observe(handler)]);
  assert.equal(startCount, 1);

  emit({ payload: "first" });
  assert.equal(callCount, 2);
  disposeFirst();
  emit({ payload: "second" });
  assert.equal(callCount, 3);
  assert.equal(stopCount, 0);

  disposeSecond();
  disposeSecond();
  assert.equal(stopCount, 1);
});
