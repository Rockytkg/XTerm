import { useI18n } from "vue-i18n";
import { SCRIPT_RUN_STATUS, runScript } from "../services/scripting/scriptRunner";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useToasts } from "./useToasts";

// 把脚本引擎接到工作区会话上：负责解析目标会话与运行结果的通知。
// 输入/输出能力由终端上加载的 ScriptBridgeAddon 提供，脚本引擎按会话 id 动态获取。
export function useScriptExecution() {
  const { t } = useI18n();
  const { showToast } = useToasts();
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
    const run = await runScript(script, {
      targetSessionId: frontendSessionId,
      targetLabel: sessionLabel(frontendSessionId),
      sessionInfo: sessionInfo(frontendSessionId),
    });

    if (run.status === SCRIPT_RUN_STATUS.ERROR) {
      showToast({
        type: "error",
        title: t("notifications.scriptFailed", { name: script.name }),
        message: run.error,
      });
    } else if (run.status === SCRIPT_RUN_STATUS.DONE) {
      showToast({
        type: "success",
        title: t("notifications.scriptFinished", { name: script.name }),
      });
    }
    return run;
  }

  function runScriptOnActiveSession(script) {
    return runScriptOnSession(script, workspace.activeConnection || "");
  }

  return {
    runScriptOnActiveSession,
    runScriptOnSession,
  };
}
