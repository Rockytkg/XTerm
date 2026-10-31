// await/then/catch 会消费子类 Promise 的 then；原生 Promise 的 await 则不会经过可覆写方法。
// 利用这一差异，运行时只对脚本未消费的拒绝执行 fail，已捕获的 API 超时可以正常继续。
class ScriptTaskPromise extends Promise {
  constructor(executor) {
    super(executor);
    this.consumerHandled = false;
  }

  then(onFulfilled, onRejected) {
    if (typeof onRejected === "function") this.consumerHandled = true;
    return super.then(onFulfilled, onRejected);
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
    if (activeWorkCount() || !resolveIdle) return;
    const resolve = resolveIdle;
    resolveIdle = null;
    resolve();
  }

  function trackTask(task) {
    const promise = new ScriptTaskPromise((resolve, reject) => {
      Promise.resolve(task).then(resolve, reject);
    });
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
    },
    trackTask,
    waitForBackgroundWork,
  };
}
