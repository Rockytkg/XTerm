import { createLogger } from "../../utils/logger.js";
import { executeScript } from "./scriptExecutor.js";

const hostLogger = createLogger("frontend.scripting.host");

// 直连执行宿主：脚本在主线程内编译执行。用于 node 单测（无 Worker）与
// Worker 创建失败时的回退；与 Worker 路径共享同一执行内核（executeScript）。
function createDirectHost({ code, api, run, log, hostRuntime, formatBlockedMessage }) {
  const exec = executeScript({
    code,
    log,
    abortedState: run,
    formatBlockedMessage,
    createApi: () => api,
  });
  // 脚本侧零散异步的失败转发给主运行时，统一走 run 的失败路径。
  exec.failure.catch((error) => hostRuntime.fail(error));
  return { finished: exec.done, dispose: exec.dispose };
}

// Worker 执行宿主：脚本主体在独立 Worker 线程运行，主线程仅通过 RPC 提供
// xterm.* 能力。停止脚本 = worker.terminate()，可即时杀死死循环且不会波及
// 界面；Worker 退出后其全部内存由 JS 引擎整体回收。
function createWorkerHost({ code, api, log, formatBlockedMessage }) {
  const worker = new Worker(new URL("./scriptWorker.js", import.meta.url), { type: "module" });
  let settled = false;
  let resolveFinished = null;
  let rejectFinished = null;
  const finished = new Promise((resolve, reject) => {
    resolveFinished = resolve;
    rejectFinished = reject;
  });

  function respondResult(id, payload) {
    try {
      worker.postMessage({ type: "result", id, ...payload });
    } catch {
      // 返回值无法结构化克隆（如出现函数句柄）时按调用失败回报，不让主线程抛错。
      try {
        worker.postMessage({
          type: "result",
          id,
          ok: false,
          name: "DataCloneError",
          message: "script api result is not serializable",
        });
      } catch {
        // Worker 已终止：结果无处可去，直接丢弃。
      }
    }
  }

  worker.onmessage = (event) => {
    const message = event.data || {};
    if (message.type === "call") {
      const fn = api[message.method];
      if (typeof fn !== "function") {
        respondResult(message.id, {
          ok: false,
          name: "Error",
          message: `unknown script api: ${String(message.method)}`,
        });
        return;
      }
      Promise.resolve()
        .then(() => fn(...(Array.isArray(message.args) ? message.args : [])))
        .then(
          (value) => respondResult(message.id, { ok: true, value }),
          (error) =>
            respondResult(message.id, {
              ok: false,
              name: error?.name || "Error",
              message: String(error?.message || error || "script api call failed"),
            }),
        );
      return;
    }
    if (message.type === "log") {
      log(...(Array.isArray(message.args) ? message.args : []));
      return;
    }
    if (settled) return;
    if (message.type === "done") {
      settled = true;
      resolveFinished();
    } else if (message.type === "error") {
      settled = true;
      const error = new Error(message.message || "script failed");
      error.name = message.name || "Error";
      rejectFinished(error);
    }
  };

  worker.onerror = (event) => {
    if (settled) return;
    settled = true;
    rejectFinished(new Error(event.message || "script worker crashed"));
  };

  worker.postMessage({
    type: "start",
    code,
    // 方法清单与静态会话数据随启动消息下发；Worker 侧据此生成 RPC 版 xterm 对象。
    methods: Object.keys(api).filter((key) => key !== "session"),
    session: { ...api.session },
    blockedTemplate: formatBlockedMessage("{api}"),
  });

  return {
    finished,
    dispose: () => worker.terminate(),
  };
}

export function createScriptExecutorHost(options) {
  if (typeof Worker === "function") {
    try {
      return createWorkerHost(options);
    } catch (error) {
      hostLogger.warn("script worker unavailable, falling back to direct execution:", error);
    }
  }
  return createDirectHost(options);
}
