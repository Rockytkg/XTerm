import { watch } from "vue";
import { useI18n } from "vue-i18n";
import { SCRIPT_RUN_STATUS, runScript, stopScript } from "../services/scripting/scriptRunner";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useToasts } from "./useToasts";

// 同一会话上的同一脚本共用一条 toast：执行中的 loading 提示原地更新为
// 最终结果（完成/失败/已停止），避免堆叠多条提示。
function scriptRunToastId(scriptId, sessionId) {
  return `script-run-${scriptId || "anon"}-${sessionId || "none"}`;
}

// 等待响应式 run.status 离开 RUNNING：watch 事件驱动取代 10ms 轮询，
// 300ms 上限语义与原实现一致。在事件处理器上下文创建（无活动实例），
// 两条出口都手动 stop。
const SCRIPT_RUN_STOP_TIMEOUT_MS = 300;

function waitForRunSettled(run) {
  if (run.status !== SCRIPT_RUN_STATUS.RUNNING) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      stopWatch();
      resolve();
    }, SCRIPT_RUN_STOP_TIMEOUT_MS);
    const stopWatch = watch(
      () => run.status,
      (status) => {
        if (status === SCRIPT_RUN_STATUS.RUNNING) return;
        clearTimeout(timer);
        stopWatch();
        resolve();
      },
      // finish() 同步写入 status，sync flush 保证同一拍拿到结果。
      { flush: "sync" },
    );
  });
}

// 把脚本引擎接到工作区会话上：负责解析目标会话与运行结果的通知。
// 输入/输出能力由终端上加载的 ScriptBridgeAddon 提供，脚本引擎按会话 id 动态获取。
export function useScriptExecution() {
  const { t } = useI18n();
  const { showToast, updateToast } = useToasts();
  const workspace = useWorkspaceStore();

  function sessionLabel(frontendSessionId) {
    const session = workspace.openSessions.find((item) => item.id === frontendSessionId);
    return session?.name || session?.host || frontendSessionId;
  }

  // 透传给脚本的会话元数据：只取非敏感连接信息，密码/私钥等凭证不离开凭证存储。
  function sessionInfo(frontendSessionId) {
    const session = workspace.openSessions.find((item) => item.id === frontendSessionId);
    return {
      protocol: session?.protocol || "",
      host: session?.host || "",
      port: session?.port ?? "",
      username: session?.username || "",
    };
  }

  async function runScriptOnSession(script, frontendSessionId) {
    if (!script?.code?.trim()) {
      showToast({ type: "warning", title: t("notifications.scriptEmpty") });
      return null;
    }
    if (!frontendSessionId) {
      showToast({ type: "error", title: t("notifications.scriptNoTarget") });
      return null;
    }
    const toastId = scriptRunToastId(script.id, frontendSessionId);
    showToast({
      id: toastId,
      type: "loading",
      title: t("notifications.scriptRunning", { name: script.name }),
    });
    const run = await runScript(script, {
      targetSessionId: frontendSessionId,
      targetLabel: sessionLabel(frontendSessionId),
      sessionInfo: sessionInfo(frontendSessionId),
    });

    if (run.status === SCRIPT_RUN_STATUS.ERROR) {
      updateToast(toastId, {
        type: "error",
        title: t("notifications.scriptFailed", { name: script.name }),
        message: run.error,
      });
    } else if (run.status === SCRIPT_RUN_STATUS.DONE) {
      updateToast(toastId, {
        type: "success",
        title: t("notifications.scriptFinished", { name: script.name }),
      });
    } else if (run.status === SCRIPT_RUN_STATUS.STOPPED) {
      updateToast(toastId, {
        type: "success",
        title: t("notifications.scriptStopped", { name: script.name }),
      });
    }
    return run;
  }

  // 请求中断运行中的脚本：先把该脚本的 toast 切到“正在中断”的 loading 状态，
  // 中断成功后由 runScriptOnSession 的结束分支把它更新为“已停止”。
  async function stopScriptRun(run) {
    const toastId = scriptRunToastId(run.scriptId, run.targetSessionId);
    showToast({
      id: toastId,
      type: "loading",
      title: t("notifications.scriptStopping", { name: run.scriptName }),
    });
    if (!stopScript(run.runId)) {
      updateToast(toastId, {
        type: "error",
        title: t("notifications.scriptStopFailed", { name: run.scriptName }),
      });
      return false;
    }
    await waitForRunSettled(run);
    if (run.status === SCRIPT_RUN_STATUS.RUNNING) {
      updateToast(toastId, {
        type: "error",
        title: t("notifications.scriptStopFailed", { name: run.scriptName }),
      });
      return false;
    }
    return true;
  }

  function runScriptOnActiveSession(script) {
    return runScriptOnSession(script, workspace.activeConnection || "");
  }

  return {
    runScriptOnActiveSession,
    runScriptOnSession,
    stopScriptRun,
  };
}
