import { executeScript } from "./scriptExecutor.js";

// 脚本 Worker 入口：用户代码在本线程内编译执行，主线程只承担 xterm.* 的 RPC 服务。
// Worker 天然没有 DOM / __TAURI__ 桥，且可被主线程 terminate() 强制结束——
// 恶意死循环不再冻结界面；沙盒屏蔽清单在 Worker 内依然生效，网络由 CSP 兜底。
// 协议：
//   main → worker: { type: "start", code, methods, session, blockedTemplate }
//                  { type: "result", id, ok, value | name, message }
//   worker → main: { type: "call", id, method, args } | { type: "log", args }
//                  { type: "done" } | { type: "error", name, message }

const pendingCalls = new Map();
let nextCallId = 0;

function postLog(args) {
  self.postMessage({ type: "log", args });
}

function callRemote(method, args) {
  const id = (nextCallId += 1);
  return new Promise((resolve, reject) => {
    pendingCalls.set(id, { resolve, reject });
    self.postMessage({ type: "call", id, method, args });
  });
}

function reportError(error) {
  self.postMessage({
    type: "error",
    name: error?.name || "Error",
    message: String(error?.message || error || "script failed"),
  });
}

function startRun(message) {
  const blockedTemplate = String(message.blockedTemplate || "");
  const host = executeScript({
    code: message.code,
    log: (...args) => postLog(args),
    // Worker 的生死由主线程 terminate 决定，本地无需处理 aborted。
    abortedState: { aborted: false },
    formatBlockedMessage: (name) => blockedTemplate.split("{api}").join(name),
    createApi(runtime) {
      const api = { session: Object.freeze({ ...(message.session || {}) }) };
      for (const method of message.methods || []) {
        if (method === "session") continue;
        // log 走单向消息，省去 RPC 往返；其余方法经 trackTask 托管，
        // 未被 await 的调用也会阻塞"完成"并把拒绝上报为运行失败。
        api[method] =
          method === "log"
            ? (...args) => postLog(args)
            : (...args) => runtime.trackTask(callRemote(method, args));
      }
      return api;
    },
  });
  host.done.then(
    () => self.postMessage({ type: "done" }),
    (error) => reportError(error),
  );
  host.failure.catch((error) => reportError(error));
}

self.onmessage = (event) => {
  const message = event.data || {};
  if (message.type === "result") {
    const entry = pendingCalls.get(message.id);
    if (!entry) return;
    pendingCalls.delete(message.id);
    if (message.ok) {
      entry.resolve(message.value);
    } else {
      const error = new Error(message.message || "script api call failed");
      error.name = message.name || "Error";
      entry.reject(error);
    }
    return;
  }
  if (message.type !== "start") return;
  try {
    startRun(message);
  } catch (error) {
    // 编译期错误（语法错误等）同步抛出，按运行失败上报。
    reportError(error);
  }
};
