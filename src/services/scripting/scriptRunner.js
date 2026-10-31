import { reactive, ref } from "vue";
import { i18n } from "../../i18n/index.js";
import { sliceGraphemePrefix } from "../../utils/grapheme.js";
import { createLogger } from "../../utils/logger.js";
import { createRuntimeId } from "../../utils/runtimeIds.js";
import { getScriptBridge } from "./bridges.js";
import { createScriptApi, stripAnsi } from "./scriptApi.js";
import { ScriptStoppedError } from "./scriptErrors.js";
import { createScriptRuntime } from "./scriptRuntime.js";
import { createScriptExecutorHost } from "./scriptExecutorHost.js";

// 兼容既有引用：stripAnsi 仍由运行器导出；错误类型从共享错误模块导入。
export { ScriptStoppedError, stripAnsi };

const runnerLogger = createLogger("frontend.scripting.runner");

export const SCRIPT_RUN_STATUS = Object.freeze({
  QUEUED: "queued",
  RUNNING: "running",
  DONE: "done",
  ERROR: "error",
  STOPPED: "stopped",
});

const MAX_RUN_HISTORY = 50;
const MAX_RUN_LOGS = 1000;
const MAX_QUEUED_RUNS_PER_SESSION = 20;
// 单条日志参数的字符上限：防止脚本用巨型字符串撑爆运行记录与日志渲染。
const MAX_LOG_ARG_CHARS = 8 * 1024;

// 所有运行记录（含历史），视图据此渲染运行状态与日志。
export const scriptRuns = ref([]);
const runControls = new Map();
const sessionQueues = new Map();

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
  return text.length > MAX_LOG_ARG_CHARS
    ? `${sliceGraphemePrefix(text, MAX_LOG_ARG_CHARS)}…`
    : text;
}

function appendRunLog(run, ...args) {
  if (run.logs.length >= MAX_RUN_LOGS) return;
  run.logs.push({ time: Date.now(), text: args.map(stringifyLogArg).join(" ") });
}

function errorMessage(error) {
  if (error instanceof Error) return error.message || error.name;
  return String(error || i18n.global.t("scripts.errors.unknownError"));
}

function isActiveRun(run) {
  return run.status === SCRIPT_RUN_STATUS.QUEUED || run.status === SCRIPT_RUN_STATUS.RUNNING;
}

function trimRunHistory() {
  const activeRuns = scriptRuns.value.filter(isActiveRun);
  const history = scriptRuns.value.filter((run) => !isActiveRun(run));
  const historyLimit = Math.max(0, MAX_RUN_HISTORY - activeRuns.length);
  scriptRuns.value = [...activeRuns, ...history.slice(0, historyLimit)];
}

function createRunJob(script, context, sessionId) {
  const run = reactive({
    runId: createRuntimeId(),
    scriptId: script?.id || "",
    scriptName: script?.name || "",
    targetSessionId: sessionId,
    targetLabel: context?.targetLabel || "",
    status: SCRIPT_RUN_STATUS.QUEUED,
    queuedAt: Date.now(),
    startedAt: 0,
    endedAt: 0,
    error: "",
    logs: [],
    aborted: false,
  });
  let resolve;
  const finished = new Promise((complete) => {
    resolve = complete;
  });
  return {
    run,
    script: { id: run.scriptId, name: run.scriptName, code: String(script?.code || "") },
    context: {
      ...context,
      targetSessionId: sessionId,
      targetLabel: run.targetLabel,
      sessionInfo: context?.sessionInfo ? { ...context.sessionInfo } : {},
    },
    finished,
    resolve,
    lifecycle: null,
    queue: null,
  };
}

function settleRun(job, status, error = "") {
  if (!isActiveRun(job.run)) return;
  job.run.status = status;
  job.run.error = error;
  job.run.endedAt = Date.now();
  runControls.delete(job.run.runId);
  trimRunHistory();
  job.resolve(job.run);
}

function stopQueuedRun(job) {
  if (job.run.status !== SCRIPT_RUN_STATUS.QUEUED || !job.queue) return false;
  const index = job.queue.pending.indexOf(job);
  if (index < 0) return false;
  job.queue.pending.splice(index, 1);
  settleRun(job, SCRIPT_RUN_STATUS.STOPPED);
  return true;
}

function drainSessionQueue(queue) {
  if (queue.draining) return;
  queue.draining = true;
  void (async () => {
    while (queue.pending.length) {
      const job = queue.pending.shift();
      if (job.run.status !== SCRIPT_RUN_STATUS.QUEUED) continue;
      job.run.status = SCRIPT_RUN_STATUS.RUNNING;
      job.run.startedAt = Date.now();
      try {
        await executeScript(job);
      } catch (error) {
        runnerLogger.error("run.unhandled.failure", error);
        settleRun(job, SCRIPT_RUN_STATUS.ERROR, errorMessage(error));
      }
    }
  })()
    .catch((error) => runnerLogger.error("queue.failed", error))
    .finally(() => {
      queue.draining = false;
      if (queue.pending.length) {
        drainSessionQueue(queue);
      } else if (sessionQueues.get(queue.sessionId) === queue) {
        sessionQueues.delete(queue.sessionId);
      }
    });
}

/**
 * 在指定终端会话上执行脚本。
 * @param {object} script { id, name, code }
 * @param {object} context { targetSessionId: 前端会话（tab）id, targetLabel: 展示用会话名 }
 * @returns {Promise<object>} 运行记录（status 为 done/error/stopped）
 */
export function runScript(script, context) {
  const sessionId = context?.targetSessionId || "";
  const job = createRunJob(script, context, sessionId);
  scriptRuns.value = [job.run, ...scriptRuns.value];
  trimRunHistory();

  if (!sessionId) {
    settleRun(job, SCRIPT_RUN_STATUS.ERROR, i18n.global.t("scripts.errors.targetUnavailable"));
    return job.finished;
  }

  let queue = sessionQueues.get(sessionId);
  if (!queue) {
    queue = { sessionId, pending: [], draining: false };
    sessionQueues.set(sessionId, queue);
  }
  if (queue.pending.length >= MAX_QUEUED_RUNS_PER_SESSION) {
    settleRun(job, SCRIPT_RUN_STATUS.ERROR, i18n.global.t("scripts.errors.queueFull"));
    return job.finished;
  }

  job.queue = queue;
  runControls.set(job.run.runId, {
    stop() {
      if (stopQueuedRun(job)) return true;
      if (job.run.status !== SCRIPT_RUN_STATUS.RUNNING) return false;
      job.run.aborted = true;
      job.lifecycle?.failPending?.();
      job.lifecycle?.failRuntime?.(new ScriptStoppedError());
      return true;
    },
  });
  queue.pending.push(job);
  drainSessionQueue(queue);
  return job.finished;
}

async function executeScript(job) {
  const { run, script, context } = job;
  const lifecycle = {
    cleanups: [],
    failPending: null,
    failRuntime: null,
    finished: false,
  };
  job.lifecycle = lifecycle;

  const finish = async (status, error = "") => {
    if (lifecycle.finished) return;
    lifecycle.finished = true;
    for (const cleanup of lifecycle.cleanups.splice(0).reverse()) {
      try {
        await cleanup();
      } catch (cleanupError) {
        runnerLogger.warn("run.cleanup.failed", cleanupError);
      }
    }
    settleRun(job, status, error);
  };

  if (!run.targetSessionId || !getScriptBridge(run.targetSessionId)) {
    await finish(SCRIPT_RUN_STATUS.ERROR, i18n.global.t("scripts.errors.targetUnavailable"));
    return run;
  }

  try {
    // 主线程运行时：托管 API 侧的后台调用（未被 await 的 xterm.* 也纳入完成判定）；
    // 脚本主体由 Worker 运行，停止时可 terminate；仅 Node 测试环境使用直连执行。
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
    runnerLogger.info("run.started", { script: run.scriptName, target: run.targetLabel });

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
      runnerLogger.warn("run.failed", error);
      await finish(SCRIPT_RUN_STATUS.ERROR, errorMessage(error));
    }
  }
  return run;
}

export function stopScript(runId) {
  const control = runControls.get(runId);
  return control?.stop() === true;
}
