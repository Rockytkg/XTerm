import { createCredential, loadCredentials, updateCredential } from "../services/credentials";
import { loadCredentialContext } from "../services/credentialContext";
import { authenticateBackendConnection } from "../services/terminalSessions";
import {
  getConnection as getConnectionProfile,
  updateConnectionProfile,
} from "../services/workspace";
import { createLogger } from "../utils/logger";
import {
  buildCredentialTypeChangeImpact,
  CREDENTIAL_TYPE_CHANGE_ACTION,
} from "../utils/credentialTypeChange";
import { CONNECTION_EVENT } from "./connectionStateMachine";
import { isCredentialPromptError } from "./sshCredentialPromptController";
import { resolveTerminalGeometry } from "./terminalGeometry";
import { formatConnectionError } from "./workspaceUtils";

const logger = createLogger("frontend.workspace.ssh_flow");

function credentialNameFor(connection, authMethod) {
  const protocol = String(connection?.protocol || "ssh").toUpperCase();
  const base = connection?.name || connection?.host || protocol;
  return `${base} ${authMethod === "key" ? "key" : "password"}`;
}

function sshCredentialOverride(input) {
  if (input.authMethod === "key") {
    return {
      authMethod: "key",
      username: input.username,
      privateKey: input.privateKey,
      passphrase: input.passphrase || undefined,
    };
  }
  return {
    authMethod: "password",
    username: input.username,
    password: input.password,
  };
}

function credentialPayload(input, name) {
  return {
    credType: input.authMethod,
    name,
    password: input.authMethod === "password" ? input.password : undefined,
    privateKey: input.authMethod === "key" ? input.privateKey : undefined,
    passphrase: input.authMethod === "key" ? input.passphrase : undefined,
  };
}

async function credentialNameById(credentialId, fallback) {
  if (!credentialId) return fallback;
  try {
    const credentials = await loadCredentials();
    return credentials.find((credential) => credential.id === credentialId)?.name || fallback;
  } catch (error) {
    logger.warn("credential_prompt.metadata_lookup_failed", {
      credentialId,
      error: String(error),
    });
    return fallback;
  }
}

function resolvePersistence(input, prompt) {
  if (prompt.saveAllowed === false) return "temporary";
  if (input.credentialPersistence === CREDENTIAL_TYPE_CHANGE_ACTION.CREATE_NEW) {
    return "createNew";
  }
  if (input.credentialPersistence === "createNew") return "createNew";
  if (input.credentialPersistence === "updateExisting" && prompt.currentCredentialId) {
    return "updateExisting";
  }
  return "temporary";
}

async function loadCredentialChangeContext(credentialId) {
  const context = await loadCredentialContext();
  const id = String(credentialId || "");
  return {
    credential: context.credentials.find((item) => item.id === id),
    connections: context.connections,
    usages: context.usages.filter((usage) => usage.credentialId === id),
  };
}

export function connectionPatchForCredential(profile, input, credentialId) {
  return {
    ...profile,
    user: input.username,
    details: {
      ...profile.details,
      protocol: profile.protocol,
      authMethod: input.authMethod,
      savedCredentialId: credentialId,
    },
  };
}

function credentialRetryOptions(connection, input) {
  if (connection.protocol === "vnc") return { vncPassword: input.password };
  if (connection.protocol === "rdp") return { rdpPassword: input.password };
  return { sshCredential: sshCredentialOverride(input) };
}

export function createSshConnectionFlow({
  activeTerminalSize,
  connectionRuntime,
  dispatchConnectionEvent,
  finishConnectionAttempt,
  getConnection,
  getSessionInstance,
  hostKeyPromptController,
  isExpectedCloseError,
  refreshConnectionList,
  reconnectSession,
  onAuthenticatedResponse,
  requestClose,
  sshCredentialPromptController,
}) {
  const pendingCredentials = new Map();

  function frontendSessionIdFor(connectionId, context = {}) {
    return context.sessionId || connectionId;
  }

  function prepareOpenOptions(connectionId, options = {}) {
    const frontendSessionId = frontendSessionIdFor(connectionId, options);
    if (options.sshCredential) {
      pendingCredentials.set(frontendSessionId, options.sshCredential);
      return { ...options, sshCredential: options.sshCredential };
    }
    pendingCredentials.delete(frontendSessionId);
    return { ...options };
  }

  function clearConnection(connectionId, sessionId = "") {
    hostKeyPromptController.cancelPromptForConnection(connectionId, sessionId);
    sshCredentialPromptController?.cancelPromptForConnection(connectionId, sessionId);
    if (sessionId) pendingCredentials.delete(sessionId);
  }

  function handleOpenResponse(connectionId, response, context = {}) {
    const frontendSessionId = frontendSessionIdFor(connectionId, context);
    if (response?.awaiting !== "hostKeyChallenge") {
      pendingCredentials.delete(frontendSessionId);
    }
  }

  function handleOpenError(connectionId, error, context = {}) {
    const formatted = formatConnectionError(error);
    const frontendSessionId = frontendSessionIdFor(connectionId, context);
    const connection = getConnection(connectionId);
    if (
      connectionRuntime.isCurrent(frontendSessionId, context.attemptToken) &&
      isCredentialPromptError(formatted, connection?.protocol)
    ) {
      pendingCredentials.delete(frontendSessionId);
      return (
        sshCredentialPromptController?.setPromptForError(connectionId, formatted, {
          attemptToken: context.attemptToken,
          sessionId: context.sessionId || "",
        }) ?? false
      );
    }
    // 非凭证弹窗的失败路径没有别的清理点，明文凭证不能滞留到关 tab。
    pendingCredentials.delete(frontendSessionId);
    return false;
  }

  function requestCredentialPrompt(sessionId, error, context = {}) {
    const connectionId = getSessionInstance?.(sessionId)?.connectionId || "";
    if (!connectionId) return false;
    return (
      sshCredentialPromptController?.setPromptForError(connectionId, formatConnectionError(error), {
        ...context,
        sessionId,
      }) ?? false
    );
  }

  async function saveCredentialSelection(connectionId, profile) {
    const details = profile?.details ?? {};
    const credentialId = details.savedCredentialId;
    const current = getConnection(connectionId);
    const profileChanged =
      !current ||
      current.user !== profile.user ||
      current.authMethod !== details.authMethod ||
      current.savedCredentialId !== credentialId;

    if (profileChanged) {
      await updateConnectionProfile(connectionId, profile);
    }
    await refreshConnectionList();
  }

  async function persistCredentialPrompt(prompt, input, persistence) {
    const connectionId = prompt.connectionId;
    if (prompt.sessionId && !getSessionInstance?.(prompt.sessionId)) return false;
    const current = getConnection(connectionId);
    if (!current) return false;

    if (persistence === "createNew") {
      const credential = await createCredential(
        credentialPayload(input, credentialNameFor(current, input.authMethod)),
      );
      const profile = await getConnectionProfile(connectionId);
      await saveCredentialSelection(
        connectionId,
        connectionPatchForCredential(profile, input, credential.id),
      );
    } else if (persistence === "updateExisting" && prompt.currentCredentialId) {
      const fallbackName = credentialNameFor(current, input.authMethod);
      const name = await credentialNameById(prompt.currentCredentialId, fallbackName);
      const credential = await updateCredential({
        id: prompt.currentCredentialId,
        ...credentialPayload(input, name),
      });
      const profile = await getConnectionProfile(connectionId);
      await saveCredentialSelection(
        connectionId,
        connectionPatchForCredential(profile, input, credential.id),
      );
    }

    if (prompt.onAnswer) {
      await prompt.onAnswer(input);
    } else if (persistence === "temporary") {
      reconnectSession(prompt.sessionId, credentialRetryOptions(current, input));
    } else {
      reconnectSession(prompt.sessionId);
    }
    return true;
  }

  async function maybeRequestTypeChangeConfirmation(prompt, input, persistence) {
    if (persistence !== "updateExisting" || input.typeChangeAction || !prompt.currentCredentialId) {
      return false;
    }
    const context = await loadCredentialChangeContext(prompt.currentCredentialId);
    const impact = buildCredentialTypeChangeImpact({
      credential: context.credential,
      nextType: input.authMethod,
      usages: context.usages,
      connections: context.connections,
    });
    if (!impact.needsConfirmation) return false;
    sshCredentialPromptController?.updatePrompt({
      ...prompt,
      typeChangeConfirm: impact,
      pendingInput: input,
    });
    return true;
  }

  async function answerCredentialPrompt(input) {
    const prompt = sshCredentialPromptController?.answerPrompt();
    if (!prompt) return false;
    const connectionId = prompt.connectionId;
    const current = getConnection(connectionId);
    if (!current) return false;
    const persistence = resolvePersistence(input, prompt);

    logger.info("credential_prompt.answered", {
      connectionId,
      persistence,
      authMethod: input.authMethod,
    });

    try {
      if (await maybeRequestTypeChangeConfirmation(prompt, input, persistence)) return true;
      const persisted = await persistCredentialPrompt(prompt, input, persistence);
      if (!persisted) {
        // 会话实例已消失（用户已关掉 tab）：弹窗已被 answerPrompt 关闭，
        // 但不能再继续重连；按失败口径补发状态机事件并返回 false。
        logger.warn("credential_prompt.submit_aborted", {
          connectionId,
          reason: "session-gone",
        });
        dispatchConnectionEvent(eventTargetForPrompt(prompt), {
          type: CONNECTION_EVENT.OPEN_FAILED,
          payload: { error: prompt.error },
        });
        return false;
      }
      return true;
    } catch (error) {
      logger.error("credential_prompt.submit_failed", error);
      dispatchConnectionEvent(eventTargetForPrompt(prompt), {
        type: CONNECTION_EVENT.OPEN_FAILED,
        payload: { error: formatConnectionError(error) },
      });
      return false;
    }
  }

  function cancelCredentialPrompt() {
    const prompt = sshCredentialPromptController?.answerPrompt();
    if (!prompt) return false;
    logger.warn("credential_prompt.cancelled", {
      connectionId: prompt.connectionId,
      reason: prompt.reason,
    });
    if (prompt.sessionId) pendingCredentials.delete(prompt.sessionId);
    dispatchConnectionEvent(eventTargetForPrompt(prompt), {
      type: CONNECTION_EVENT.OPEN_FAILED,
      payload: { error: prompt.error },
    });
    return true;
  }

  function answerHostKeyPrompt(mode) {
    const answered = hostKeyPromptController.answerPrompt(mode);
    if (!answered?.prompt) return false;
    const { prompt, mode: resolvedMode } = answered;
    if (resolvedMode === "cancel") {
      logger.warn("host_key_prompt.cancelled", {
        connectionId: prompt.connectionId,
      });
      requestClose(prompt.connectionId, {
        openRequestId: prompt.openRequestId || prompt.sessionId || prompt.connectionId,
      }).catch((error) => {
        if (!isExpectedCloseError(error)) {
          logger.error("Failed to cancel pending SSH host-key connection", error);
        }
      });
      // 不能调用 connectionRuntime.cancel：cancel 进入 closing 相位，只有
      // closeSession 的 finally 会用 closeComplete 收尾；此处后端 pending open
      // 已由 terminal_connection_open_cancel 丢弃，没有进行中的会话关闭，
      // 必须用 finish 直接结束本次 attempt，否则 closing 残留会让后续
      // 回车重连被 connection.begin 拒绝（弹窗不再出现、终端无输出）。
      connectionRuntime.finish(prompt.sessionId || prompt.connectionId, prompt.attemptToken);
      finishConnectionAttempt?.(prompt.sessionId || prompt.connectionId, prompt.attemptToken);
      if (prompt.sessionId) pendingCredentials.delete(prompt.sessionId);
      dispatchConnectionEvent(prompt.sessionId || prompt.connectionId, {
        type: CONNECTION_EVENT.HOST_KEY_CANCELLED,
      });
      return true;
    }

    logger.info("host_key_prompt.answered", {
      connectionId: prompt.connectionId,
      mode: resolvedMode,
    });
    const frontendSessionId = prompt.sessionId || prompt.connectionId;
    const backendOpenRequestId = prompt.openRequestId || frontendSessionId;
    if (!connectionRuntime.isCurrent(frontendSessionId, prompt.attemptToken)) {
      pendingCredentials.delete(frontendSessionId);
      return false;
    }
    authenticateBackendConnection(prompt.connectionId, {
      openRequestId: backendOpenRequestId,
      trustHostKey: resolvedMode === "save",
      acceptHostKeyOnce: resolvedMode === "once",
      sshCredential: pendingCredentials.get(frontendSessionId),
      ...resolveTerminalGeometry(activeTerminalSize),
    })
      .then((response) => {
        onAuthenticatedResponse?.(prompt.connectionId, response, {
          attemptToken: prompt.attemptToken,
          sessionId: prompt.sessionId || "",
        });
        pendingCredentials.delete(prompt.sessionId || prompt.connectionId);
      })
      .catch((error) => {
        logger.error("Failed to answer host key prompt", error);
        pendingCredentials.delete(prompt.sessionId || prompt.connectionId);
        const context = {
          attemptToken: prompt.attemptToken,
          sessionId: prompt.sessionId || "",
        };
        if (!handleOpenError(prompt.connectionId, error, context)) {
          dispatchConnectionEvent(frontendSessionId, {
            type: CONNECTION_EVENT.HOST_KEY_AUTH_FAILED,
            payload: { error: formatConnectionError(error) },
          });
        }
      })
      .finally(() => {
        finishConnectionAttempt?.(frontendSessionId, prompt.attemptToken);
        connectionRuntime.finish(frontendSessionId, prompt.attemptToken);
      });
    return true;
  }

  return {
    answerCredentialPrompt,
    answerHostKeyPrompt,
    cancelCredentialPrompt,
    clearConnection,
    handleOpenError,
    handleOpenResponse,
    prepareOpenOptions,
    requestCredentialPrompt,
  };
}
function eventTargetForPrompt(prompt) {
  return prompt?.sessionId || prompt?.connectionId || "";
}
