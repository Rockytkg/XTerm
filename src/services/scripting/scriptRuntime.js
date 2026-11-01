// await/then/catch 会消费子类 Promise 的 then；原生 Promise 的 await 则不会经过可覆写方法。
// 利用这一差异，运行时只对脚本未消费的拒绝执行 fail，已捕获的 API 超时可以正常继续。
class ScriptTaskPromise extends Promise {
  constructor(executor, tracker = null) {
    super(executor);
    this.consumerHandled = false;
    this.tracker = tracker;
  }

  // Promise.prototype.then/catch/finally create a child promise through the
  // species constructor. Return a native child here and explicitly register
  // it with the runtime so every chain remains part of the idle barrier.
  static get [Symbol.species]() {
    return Promise;
  }

  then(onFulfilled, onRejected) {
    if (typeof onRejected === "function") this.consumerHandled = true;
    const child = super.then(onFulfilled, onRejected);
    return this.tracker ? this.tracker(child) : child;
  }
}

// 脚本运行时作用域：托管定时器/微任务/后台 Promise 与 console，
// 让脚本体内的"零散异步"也被追踪——后台工作未结束不算完成，回调抛错会终止运行。
// 纯逻辑、无框架依赖：主线程（runner/直连执行）与 Web Worker（scriptWorker）共用。
export function createScriptRuntime(run, log) {
  const timeouts = new Set();
  const intervals = new Set();
  const backgroundTasks = new Set();
  let rejectFailure = null;
  let idlePromise = Promise.resolve();
  let resolveIdle = null;
  let idleCheckScheduled = false;
  let settled = false;

  const failure = new Promise((_, reject) => {
    rejectFailure = reject;
  });

  function fail(error) {
    if (settled) return;
    settled = true;
    rejectFailure(error instanceof Error ? error : new Error(String(error || "script failed")));
  }

  function activeWorkCount() {
    return timeouts.size + intervals.size + backgroundTasks.size;
  }

  function markBusy() {
    if (resolveIdle) return;
    idlePromise = new Promise((resolve) => {
      resolveIdle = resolve;
    });
  }

  function notifyIdle() {
    if (activeWorkCount() || !resolveIdle || idleCheckScheduled) return;
    // Promise continuations run as microtasks.  Defer the idle decision until
    // the current callback and all continuations it released have had a chance
    // to enqueue more tracked work.
    idleCheckScheduled = true;
    globalThis.queueMicrotask(() => {
      idleCheckScheduled = false;
      if (activeWorkCount() || !resolveIdle) return;
      const resolve = resolveIdle;
      resolveIdle = null;
      resolve();
    });
  }

  function trackTask(task) {
    const promise = new ScriptTaskPromise((resolve, reject) => {
      // Bypass the overridden `then` when assimilating an already tracked
      // promise. Runtime-owned handlers must not count as user consumers or
      // create a second tracked child for the same task.
      const source =
        task instanceof ScriptTaskPromise
          ? new globalThis.Promise((settle, fail) =>
              globalThis.Promise.prototype.then.call(task, settle, fail),
            )
          : globalThis.Promise.resolve(task);
      source.then(resolve, reject);
    }, trackTask);
    markBusy();
    backgroundTasks.add(promise);
    // 绕过 consumerHandled 标记：这两个处理器只属于运行时自身，不代表脚本处理了错误。
    Promise.prototype.then.call(promise, null, (error) => {
      if (!promise.consumerHandled) fail(error);
    });
    const cleanup = () => {
      backgroundTasks.delete(promise);
      notifyIdle();
    };
    Promise.prototype.then.call(promise, cleanup, cleanup);
    return promise;
  }

  function invokeCallback(callback, args) {
    if (run.aborted) return;
    try {
      trackTask(callback(...args));
    } catch (error) {
      fail(error);
    }
  }

  function runtimeSetTimeout(callback, delay = 0, ...args) {
    if (typeof callback !== "function")
      throw new TypeError("setTimeout callback must be a function");
    const handle = globalThis.setTimeout(
      () => {
        timeouts.delete(handle);
        invokeCallback(callback, args);
        notifyIdle();
      },
      Math.max(0, Number(delay) || 0),
    );
    markBusy();
    timeouts.add(handle);
    return handle;
  }

  function runtimeClearTimeout(handle) {
    timeouts.delete(handle);
    globalThis.clearTimeout(handle);
    notifyIdle();
  }

  function runtimeSetInterval(callback, delay = 0, ...args) {
    if (typeof callback !== "function") {
      throw new TypeError("setInterval callback must be a function");
    }
    const handle = globalThis.setInterval(
      () => invokeCallback(callback, args),
      Math.max(0, Number(delay) || 0),
    );
    markBusy();
    intervals.add(handle);
    return handle;
  }

  function runtimeClearInterval(handle) {
    intervals.delete(handle);
    globalThis.clearInterval(handle);
    notifyIdle();
  }

  function runtimeQueueMicrotask(callback) {
    if (typeof callback !== "function")
      throw new TypeError("queueMicrotask callback must be a function");
    trackTask(
      new Promise((resolve, reject) => {
        globalThis.queueMicrotask(() => {
          if (run.aborted) {
            resolve();
            return;
          }
          try {
            Promise.resolve(callback()).then(resolve, reject);
          } catch (error) {
            reject(error);
          }
        });
      }),
    );
  }

  function createRuntimePromise(executor) {
    if (!new.target) {
      throw new TypeError("Promise constructor cannot be invoked without 'new'");
    }
    if (typeof executor !== "function") {
      throw new TypeError("Promise resolver is not a function");
    }
    return trackTask(new globalThis.Promise(executor));
  }

  function createPromiseStatic(method) {
    return (...args) => trackTask(globalThis.Promise[method](...args));
  }

  const runtimePromise = Object.assign(createRuntimePromise, {
    prototype: globalThis.Promise.prototype,
    all: createPromiseStatic("all"),
    allSettled: createPromiseStatic("allSettled"),
    any: createPromiseStatic("any"),
    race: createPromiseStatic("race"),
    reject: createPromiseStatic("reject"),
    resolve: createPromiseStatic("resolve"),
  });
  if (typeof globalThis.Promise.withResolvers === "function") {
    runtimePromise.withResolvers = () => {
      let resolve;
      let reject;
      const promise = createRuntimePromise((resolveValue, rejectValue) => {
        resolve = resolveValue;
        reject = rejectValue;
      });
      return { promise, resolve, reject };
    };
  }

  function waitForBackgroundWork() {
    return idlePromise;
  }

  function dispose() {
    settled = true;
    for (const handle of timeouts) globalThis.clearTimeout(handle);
    for (const handle of intervals) globalThis.clearInterval(handle);
    timeouts.clear();
    intervals.clear();
    backgroundTasks.clear();
    resolveIdle?.();
    resolveIdle = null;
  }

  const runtimeConsole = new Proxy(Object.create(null), {
    get(_, property) {
      if (property === "log" || property === "info" || property === "debug") {
        return (...args) => log(...args);
      }
      if (property === "warn") return (...args) => log("[warn]", ...args);
      if (property === "error") return (...args) => log("[error]", ...args);
      const native = globalThis.console?.[property];
      return typeof native === "function" ? native.bind(globalThis.console) : native;
    },
  });

  return {
    dispose,
    fail,
    failure,
    scope: {
      console: runtimeConsole,
      setTimeout: runtimeSetTimeout,
      clearTimeout: runtimeClearTimeout,
      setInterval: runtimeSetInterval,
      clearInterval: runtimeClearInterval,
      queueMicrotask: runtimeQueueMicrotask,
      Promise: runtimePromise,
    },
    trackTask,
    waitForBackgroundWork,
  };
}
