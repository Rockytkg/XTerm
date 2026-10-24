import assert from "node:assert/strict";
import { register } from "node:module";
import test, { mock } from "node:test";

register("./helpers/mockSftpServiceLoader.mjs", import.meta.url);

const { createAsyncListenerLifecycle } = await import("../src/utils/asyncListeners.js");

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

test("attach→detach→attach 在首次订阅在途时保留最终挂载状态", async () => {
  const subscription = deferred();
  const unlisten = mock.fn();
  const subscribe = mock.fn(() => subscription.promise);
  const lifecycle = createAsyncListenerLifecycle({ subscribe });

  const firstAttach = lifecycle.attach();
  lifecycle.detach();
  const secondAttach = lifecycle.attach();
  assert.equal(secondAttach, firstAttach);
  assert.equal(subscribe.mock.calls.length, 1);

  subscription.resolve(unlisten);
  await firstAttach;
  await settle();
  assert.equal(unlisten.mock.calls.length, 0);

  lifecycle.detach();
  assert.equal(unlisten.mock.calls.length, 1);
  lifecycle.dispose();
  assert.equal(unlisten.mock.calls.length, 1);
});

test("detach 后完成的在途订阅会最终取消监听", async () => {
  const subscription = deferred();
  const unlisten = mock.fn();
  const lifecycle = createAsyncListenerLifecycle({ subscribe: () => subscription.promise });

  const attaching = lifecycle.attach();
  lifecycle.detach();
  subscription.resolve(unlisten);
  await attaching;
  await settle();

  assert.equal(unlisten.mock.calls.length, 1);
  lifecycle.dispose();
  assert.equal(unlisten.mock.calls.length, 1);
});

test("dispose 后完成的在途订阅会取消监听且不能重新 attach", async () => {
  const subscription = deferred();
  const unlisten = mock.fn();
  const subscribe = mock.fn(() => subscription.promise);
  const lifecycle = createAsyncListenerLifecycle({ subscribe });

  const attaching = lifecycle.attach();
  lifecycle.dispose();
  subscription.resolve(unlisten);
  await attaching;
  await settle();

  assert.equal(unlisten.mock.calls.length, 1);
  await lifecycle.attach();
  assert.equal(subscribe.mock.calls.length, 1);
});

test("同步订阅仍在 attach Promise 在途时 dispose 只取消一次", async () => {
  const unlisten = mock.fn();
  const lifecycle = createAsyncListenerLifecycle({ subscribe: () => unlisten });

  const attaching = lifecycle.attach();
  lifecycle.dispose();
  await attaching;
  await settle();

  assert.equal(unlisten.mock.calls.length, 1);
});
