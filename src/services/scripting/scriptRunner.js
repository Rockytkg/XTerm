import { reactive, ref } from "vue";
import { i18n } from "../../i18n/index.js";
import { createLogger } from "../../utils/logger.js";
import { createRuntimeId } from "../../utils/runtimeIds.js";
import { getScriptBridge } from "./bridges.js";
import { createScriptApi, ScriptStoppedError, stripAnsi } from "./scriptApi.js";
import { createScriptRuntime } from "./scriptRuntime.js";
import { createScriptExecutorHost } from "./scriptExecutorHost.js";

// 兼容既有引用：stripAnsi / ScriptStoppedError 的实现在 scriptApi.js。
export { ScriptStoppedError, stripAnsi };

const runnerLogger = createLogger("frontend.scripting.runner");

export const SCRIPT_RUN_STATUS = Object.freeze({
  RUNNING: "running",
  DONE: "done",
  ERROR: "error",
  STOPPED: "stopped",
});

const MAX_RUN_HISTORY = 50;
const MAX_RUN_LOGS = 1000;
// 单条日志参数的字符上限：防止脚本用巨型字符串撑爆运行记录与日志渲染。
const MAX_LOG_ARG_CHARS = 8 * 1024;

// 所有运行记录（含历史），视图据此渲染运行状态与日志。
export const scriptRuns = ref([]);
const runControls = new Map();

function stringifyLogArg(value) {
  let text;
  if (typeof value === "string") {
    text = value;
  } else {
    try {
      text = JSON.stringify(value) ?? String(value);
    } catch {
      text = String(value);
    }
  }
  return text.length > MAX_LOG_ARG_CHARS ? `${text.slice(0, MAX_LOG_ARG_CHARS)}…` : text;
}

function appendRunLog(run, ...args) {
  if (run.logs.length >= MAX_RUN_LOGS) return;
  run.logs.push({ time: Date.now(), text: args.map(stringifyLogArg).join(" ") });
}

function errorMessage(error) {
  if (error instanceof Error) return error.message || error.name;
  return String(error || "Unknown script error");
}

/**
 * 在指定终端会话上执行脚本。
 * @param {object} script { id, name, code }
 * @param {object} context { targetSessionId: 前端会话（tab）id, targetLabel: 展示用会话名 }
 * @returns {Promise<object>} 运行记录（status 为 done/error/stopped）
 */
export async function runScript(script, context) {
  // reactive：运行中 status/logs 的变更需要实时反映到视图。
  const run = reactive({
    runId: createRuntimeId(),
    scriptId: script?.id || "",
    scriptName: script?.name || "",
    targetSessionId: context?.targetSessionId || "",
    targetLabel: context?.targetLabel || "",
    status: SCRIPT_RUN_STATUS.RUNNING,
    startedAt: Date.now(),
    endedAt: 0,
    error: "",
    logs: [],
    aborted: false,
  });
  scriptRuns.value = [run, ...scriptRuns.value].slice(0, MAX_RUN_HISTORY);

  const lifecycle = {
    cleanups: [],
    failPending: null,
    failRuntime: null,
    finished: false,
  };
  runControls.set(run.runId, {
    stop() {
      run.aborted = true;
      lifecycle.failPending?.();
      lifecycle.failRuntime?.(new ScriptStoppedError());
    },
  });

  const finish = async (status, error = "") => {
    if (lifecycle.finished) return;
    lifecycle.finished = true;
    run.status = status;
    run.error = error;
    run.endedAt = Date.now();
    runControls.delete(run.runId);
    for (const cleanup of lifecycle.cleanups.splice(0).reverse()) {
      try {
        await cleanup();
      } catch (cleanupError) {
        runnerLogger.warn("script cleanup failed:", cleanupError);
      }
    }
  };

  if (!run.targetSessionId || !getScriptBridge(run.targetSessionId)) {
    await finish(SCRIPT_RUN_STATUS.ERROR, "target session is not available");
    return run;
  }

  try {
    // 主线程运行时：托管 API 侧的后台调用（未被 await 的 xterm.* 也纳入完成判定）；
    // 脚本主体由执行宿主运行——默认在独立 Worker 线程（死循环可 terminate 秒杀，
    // 内存随 Worker 整体回收），无 Worker 环境（node 单测）回退到直连执行。
    const log = (...args) => appendRunLog(run, ...args);
    const runtime = createScriptRuntime(run, log);
    lifecycle.cleanups.push(runtime.dispose);
    lifecycle.failRuntime = runtime.fail;
    const api = createScriptApi({
      run,
      context,
      lifecycle,
      trackTask: runtime.trackTask,
      log,
    });
    runnerLogger.info("script started", { script: run.scriptName, target: run.targetLabel });

    const host = createScriptExecutorHost({
      code: script?.code || "",
      api,
      run,
      hostRuntime: runtime,
      log,
      formatBlockedMessage: (name) => i18n.global.t("scripts.errors.blockedApi", { api: name }),
    });
    lifecycle.cleanups.push(host.dispose);

    const execution = (async () => {
      await host.finished;
      await runtime.waitForBackgroundWork();
    })();
    await Promise.race([execution, runtime.failure]);
    await finish(run.aborted ? SCRIPT_RUN_STATUS.STOPPED : SCRIPT_RUN_STATUS.DONE);
  } catch (error) {
    // Worker 路径的错误经消息通道返回，ScriptStoppedError 只能靠 name 识别。
    if (
      run.aborted ||
      error instanceof ScriptStoppedError ||
      error?.name === "ScriptStoppedError"
    ) {
      await finish(SCRIPT_RUN_STATUS.STOPPED);
    } else {
      runnerLogger.warn("script failed:", error);
      await finish(SCRIPT_RUN_STATUS.ERROR, errorMessage(error));
    }
  }
  return run;
}

export function stopScript(runId) {
  const run = scriptRuns.value.find((item) => item.runId === runId) || null;
  if (!run || run.status !== SCRIPT_RUN_STATUS.RUNNING) return false;
  runControls.get(runId)?.stop();
  return true;
}
