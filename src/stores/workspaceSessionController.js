import {
  createConnection,
  deleteConnection,
  getConnection as getConnectionProfile,
  updateConnectionProfile,
} from "../services/workspace";
import {
  cancelBackendConnectionOpen,
  closeBackendSession,
  deleteSshHostKey,
  redetectBackendSerialBaud,
} from "../services/terminalSessions";
import { isConnectionNotActiveError } from "../services/ipc/errors";
import { formatConnectionError } from "./workspaceUtils";
import { createWorkspaceConnectionOpener } from "./workspaceConnectionOpener";
import { applyOpenResponseMetadata } from "./workspaceRuntimeSync";
import { createLogger } from "../utils/logger";
import { CONNECTION_EVENT } from "./connectionStateMachine";
import { createSshConnectionFlow } from "./sshConnectionFlow";
import { capabilitiesCan } from "../utils/connectionCapabilities";
import { mergeConnectionProfileOptions } from "../utils/connectionProfileOptions";
import { isSerialProtocol, requiresHostKeyVerification } from "../utils/connectionProtocols";

const logger = createLogger("frontend.workspace.session_controller");

export function createWorkspaceSessionController({
  activeTerminalSize,
  activeConnection,
  activeSessions,
  activeTab,
  connectionRuntime,
  closeSessionRecording,
  dispatchConnectionEvent,
  createSessionInstance,
  createPendingSessionInstance,
  bindSessionInstanceToTerminalSession,
  clearSessionInstanceTerminalSession,
  findSessionInstancesByConnectionId,
  getConnection,
  getSessionInstance,
  hostKeyPromptController,
  sshCredentialPromptController,
  lastSerialBaudEvent,
  patchConnection,
  preferences,
  refreshConnectionList,
  addSessionToTabOrder,
  removeSessionFromTabOrder,
  removeExternalConnection,
  removeSessionInstance,
  removeProfile,
  sessionRegistry,
  upsertExternalConnection,
  forgetRuntimeMetricsSession,
  waitForTerminalPresentation,
  cancelTerminalPresentationWait,
}) {
  const closingSessions = new Map();
  const connectionUpdateQueues = new Map();
  const optimisticConnectionPatches = new Map();
  const sshFlow = createSshConnectionFlow({
    activeTerminalSize,
    connectionRuntime,
    dispatchConnectionEvent,
    finishConnectionAttempt: sessionRegistry.finishSessionAttempt,
    getConnection,
    getSessionInstance,
    hostKeyPromptController,
    isExpectedCloseError,
    preferences,
    refreshConnectionList,
    reconnect: connectTo,
    onAuthenticatedResponse: (connectionId, response, context = {}) => {
      const frontendSessionId = context.sessionId || connectionId;
      if (!connectionRuntime.isCurrent(frontendSessionId, context.attemptToken)) {
        if (response?.sessionId) {
          closeBackendSession(response.sessionId).catch((error) => {
            logger.error("Failed to close stale authenticated SSH session", error);
          });
        }
        return;
      }
      const sessionId = response?.sessionId
        ? openFrontendSession(connectionId, response.sessionId, {
            response,
            attemptToken: context.attemptToken,
            sessionId: context.sessionId,
          })
        : "";
      if (!sessionId) return;
      applyOpenResponseMetadata({
        dispatchConnectionEvent,
        response,
        sessionId,
        sessionRegistry,
      });
    },
    requestClose: cancelBackendConnectionOpen,
    sshCredentialPromptController,
  });

  const { openConnectionInBackground } = createWorkspaceConnectionOpener({
    activeTerminalSize,
    connectionRuntime,
    dispatchConnectionEvent,
    getConnection,
    onSessionOpened: (connectionId, sessionId, context) =>
      openFrontendSession(connectionId, sessionId, context),
    onOpenError: (connectionId, error, context) =>
      sshFlow.handleOpenError(connectionId, error, context),
    onOpenResponse: (connectionId, response, context) =>
      sshFlow.handleOpenResponse(connectionId, response, context),
    preferences,
    sessionRegistry,
  });

  function firstOpenConnectionId() {
    return [...activeSessions.value].find((sessionId) => getSessionInstance(sessionId)) ?? "";
  }

  function connectionIdForSession(sessionId) {
    return getSessionInstance(sessionId)?.connectionId || "";
  }

  function openFrontendSession(connectionId, sessionId, options = {}) {
    const connection = getConnection(connectionId);
    if (!connection || !sessionId) return "";
    const pendingSessionId = options.sessionId || "";
    const session =
      pendingSessionId && getSessionInstance(pendingSessionId)
        ? getSessionInstance(pendingSessionId)
        : createSessionInstance(connectionId);
    if (!session) return "";
    if (!sessionRegistry.bindBackendSession(session.id, sessionId, options.attemptToken)) {
      clearSessionInstanceTerminalSession?.(session.id);
      closeBackendSession(sessionId).catch((error) => {
        logger.error("Failed to close an unbound backend session", error);
      });
      return "";
    }
    if (sessionRegistry.getBackendSessionId(session.id) === sessionId) {
      bindSessionInstanceToTerminalSession?.(session.id, sessionId);
    } else {
      // An early failed/closed event was replayed during binding.
      clearSessionInstanceTerminalSession?.(session.id);
    }
    activeSessions.value = new Set([...activeSessions.value, session.id]);
    addSessionToTabOrder?.(session.id);
    activeConnection.value = session.id;
    activeTab.value = options.preserveActiveTab ? activeTab.value : "shell";
    logger.info("session.frontend.opened", {
      connectionId,
      sessionId: session.id,
      connectionName: connection.name,
      protocol: connection.protocol,
    });
    return session.id;
  }

  function prepareFrontendSession(connectionId, options = {}) {
    const connection = getConnection(connectionId);
    if (!connection) return null;
    const existingSessionId = options.sessionId || "";
    const existing = existingSessionId ? getSessionInstance(existingSessionId) : null;
    const session = existing || createPendingSessionInstance?.(connectionId);
    if (!session) return null;
    activeSessions.value = new Set([...activeSessions.value, session.id]);
    addSessionToTabOrder?.(session.id);
    activeConnection.value = session.id;
    activeTab.value = options.preserveActiveTab ? activeTab.value : "shell";
    logger.info("session.frontend.pending_opened", {
      connectionId,
      sessionId: session.id,
      connectionName: connection.name,
      protocol: connection.protocol,
    });
    return { created: !existing, session };
  }

  async function addConnection(profile) {
    logger.info("connection.create.requested", {
      connectionName: profile?.name,
      protocol: profile?.protocol,
    });
    const id = await createConnection(profile);
    await refreshConnectionList();
    logger.info("connection.create.completed", {
      connectionId: id,
      connectionName: profile?.name,
      protocol: profile?.protocol,
    });
    return connectTo(id);
  }

  async function updateConnection(id, patch) {
    const current = getConnection(id);
    if (current?.external) {
      // External (deep-link) connections have no persisted profile.
      // Update the in-memory catalog directly so the UI reflects
      // per-session overrides (encoding, terminal type, backspace, etc.).
      upsertExternalConnection?.({ ...current, ...patch });
      logger.info("connection.update.external_applied", {
        connectionId: id,
        patchKeys: Object.keys(patch || {}),
      });
      return;
    }
    if (!current) {
      logger.warn("connection.update.missing", {
        connectionId: id,
      });
      return;
    }
    optimisticConnectionPatches.set(id, {
      ...(optimisticConnectionPatches.get(id) || {}),
      ...patch,
    });
    patchConnection?.(id, patch);
    logger.info("connection.update.requested", {
      connectionId: id,
      patchKeys: Object.keys(patch || {}),
    });
    const previous = connectionUpdateQueues.get(id) || Promise.resolve();
    const pending = previous
      .catch(() => {})
      .then(async () => {
        try {
          const profile = await getConnectionProfile(id);
          if (!profile) throw new Error(`Connection '${id}' not found`);
          await updateConnectionProfile(id, mergeConnectionProfileOptions(profile, patch));
          logger.info("connection.update.completed", {
            connectionId: id,
          });
        } catch (error) {
          await refreshConnectionList();
          if (connectionUpdateQueues.get(id) !== pending) {
            patchConnection?.(id, optimisticConnectionPatches.get(id));
          }
          throw error;
        }
      });
    connectionUpdateQueues.set(id, pending);
    pending.then(
      () => {
        if (connectionUpdateQueues.get(id) === pending) {
          connectionUpdateQueues.delete(id);
          optimisticConnectionPatches.delete(id);
        }
      },
      () => {
        if (connectionUpdateQueues.get(id) === pending) {
          connectionUpdateQueues.delete(id);
          optimisticConnectionPatches.delete(id);
        }
      },
    );
    return pending;
  }

  async function removeConnection(id) {
    const connection = getConnection(id);
    if (connection?.external) {
      logger.warn("connection.remove.external_redirected_to_close", {
        connectionId: id,
      });
      await Promise.all(
        findSessionInstancesByConnectionId?.(id).map((session) => closeSession(session.id)) || [],
      );
      removeExternalConnection?.(id);
      return;
    }
    logger.warn("connection.remove.requested", {
      connectionId: id,
      protocol: connection?.protocol,
      host: connection?.host,
    });
    if (requiresHostKeyVerification(connection?.protocol)) {
      await deleteSshHostKey({
        connectionId: id,
      });
    }
    await Promise.all(
      findSessionInstancesByConnectionId?.(id).map((session) => closeSession(session.id)) || [],
    );
    await deleteConnection(id);
    removeProfile?.(id);
    if (connectionIdForSession(activeConnection.value) === id) {
      activeConnection.value = firstOpenConnectionId();
    }
    logger.info("connection.remove.completed", {
      connectionId: id,
      remainingActiveSessions: activeSessions.value.size,
    });
  }

  function isExpectedCloseError(error) {
    return isConnectionNotActiveError(error);
  }

  function closeSession(id) {
    if (!id) return Promise.resolve(false);
    cancelTerminalPresentationWait?.(id);
    const pending = closingSessions.get(id);
    if (pending) return pending;
    const closingConnectionId = connectionIdForSession(id) || id;

    const closePromise = (async () => {
      const connectionId = closingConnectionId;
      const session = getSessionInstance(id);
      const terminalSessionId = session?.sessionId || "";
      connectionRuntime.cancel(id);
      // 本条路径随后会显式 closeBackendSession / cancelBackendConnectionOpen，
      // 跳过 retire 回调里的自动释放，避免重复关闭同一会话。
      sessionRegistry.unbindBackendSession(id, { releaseBackend: false });
      logger.info("session.close.requested", {
        connectionId,
        sessionId: id,
        terminalSessionId: terminalSessionId || null,
      });
      sshFlow.clearConnection(connectionId, id);
      forgetRuntimeMetricsSession?.(connectionId, terminalSessionId);
      await closeSessionRecording?.(id).catch((error) => {
        logger.error("Failed to close session recording", error);
      });
      dispatchConnectionEvent(id, {
        type: CONNECTION_EVENT.CLOSE_REQUESTED,
      });

      try {
        const closeRequest = terminalSessionId
          ? closeBackendSession(terminalSessionId)
          : cancelBackendConnectionOpen(connectionId, {
              openRequestId: sessionRegistry.getOpenRequestId(id) || id,
            });
        await closeRequest.catch((error) => {
          if (isExpectedCloseError(error)) {
            logger.debug("session.close.connection_already_inactive", {
              connectionId,
              sessionId: terminalSessionId || null,
            });
            return;
          }
          logger.error("Failed to close backend connection", error);
        });
      } finally {
        sessionRegistry.clearConnectionRuntime(id);
        activeSessions.value = new Set([...activeSessions.value].filter((s) => s !== id));
        removeSessionInstance?.(id);
        removeSessionFromTabOrder?.(id);
        if (activeConnection.value === id) {
          activeConnection.value = firstOpenConnectionId();
        }
        logger.info("session.close.completed", {
          connectionId: id,
          profileConnectionId: connectionId,
          sessionId: terminalSessionId || null,
          remainingActiveSessions: activeSessions.value.size,
        });
      }
      return true;
    })().finally(() => {
      closingSessions.delete(id);
      connectionRuntime.closeComplete(id);
    });

    closingSessions.set(id, closePromise);
    return closePromise;
  }

  function handleBackendSessionEnded(frontendSessionId) {
    // 后端会话已终结但本次打开仍停在 opening（典型场景：host-key 等待期间
    // 对端断开/超时）。沿用 closeSession 的清理口径收尾，否则 opening 残留会让
    // 迟到的 authenticate 响应通过 isCurrent 校验，host-key 弹窗与待发送的
    // 明文凭证也会一直滞留。
    if (!connectionRuntime.isPending(frontendSessionId)) return false;
    const attemptToken = sessionRegistry.getAttemptToken(frontendSessionId);
    const connectionId =
      connectionIdForSession(frontendSessionId) ||
      sessionRegistry.getConnectionId(frontendSessionId);
    sshFlow.clearConnection(connectionId, frontendSessionId);
    sessionRegistry.finishSessionAttempt(frontendSessionId, attemptToken);
    connectionRuntime.finish(frontendSessionId, attemptToken);
    return true;
  }

  function selectConnection(connectionId) {
    if (activeSessions.value.has(connectionId)) {
      logger.info("connection.select", {
        connectionId,
      });
      activeConnection.value = connectionId;
    } else {
      logger.debug("connection.select.ignored", {
        connectionId,
        reason: "inactive_session",
      });
    }
  }

  function connectTo(connectionId, options = {}) {
    const connection = getConnection(connectionId);
    if (!connection) {
      logger.warn("connection.open.missing", { connectionId });
      return false;
    }
    const isSerial = isSerialProtocol(connection?.protocol);
    const reusableSerialSession =
      isSerial && !options.forceReconnect
        ? findSessionInstancesByConnectionId?.(connectionId)?.find((session) => session.sessionId)
        : null;
    if (reusableSerialSession) {
      logger.info("connection.open.reused", {
        connectionId,
        sessionId: reusableSerialSession.id,
      });
      activeSessions.value = new Set([...activeSessions.value, reusableSerialSession.id]);
      addSessionToTabOrder?.(reusableSerialSession.id);
      activeConnection.value = reusableSerialSession.id;
      activeTab.value = options.preserveActiveTab ? activeTab.value : "shell";
      return true;
    }

    const prepared = prepareFrontendSession(connectionId, options);
    if (!prepared) {
      return false;
    }
    const frontendSessionId = prepared.session.id;
    const attemptToken = connectionRuntime.begin(frontendSessionId);
    if (attemptToken == null) {
      logger.warn("connection.open.refused", {
        connectionId,
        sessionId: frontendSessionId,
        reason: "close_in_progress",
      });
      if (prepared.created) {
        removeSessionInstance?.(frontendSessionId);
        activeSessions.value = new Set(
          [...activeSessions.value].filter((s) => s !== frontendSessionId),
        );
        removeSessionFromTabOrder?.(frontendSessionId);
      }
      return false;
    }
    const backendOpenRequestId = `${frontendSessionId}:${attemptToken}`;
    clearSessionInstanceTerminalSession?.(frontendSessionId);
    sessionRegistry.beginSessionAttempt(
      frontendSessionId,
      connectionId,
      attemptToken,
      backendOpenRequestId,
    );
    logger.info("connection.open.dispatched", {
      connectionId,
      sessionId: frontendSessionId,
    });
    const openOptions = sshFlow.prepareOpenOptions(connectionId, {
      ...options,
      attemptToken,
      openRequestId: backendOpenRequestId,
      sessionId: frontendSessionId,
      preserveActiveTab: options.preserveActiveTab,
    });
    void Promise.resolve(waitForTerminalPresentation?.(frontendSessionId)).then((presentation) => {
      if (presentation === "cancelled" || presentation === "superseded") return;
      if (!connectionRuntime.isCurrent(frontendSessionId, attemptToken)) return;
      logger.debug("connection.open.presentation_ready", {
        connectionId,
        sessionId: frontendSessionId,
        presentation: presentation || "unavailable",
      });
      openConnectionInBackground(connectionId, openOptions);
    });
    return true;
  }

  function answerHostKeyPrompt(mode) {
    return sshFlow.answerHostKeyPrompt(mode);
  }

  function answerSshCredentialPrompt(input) {
    return sshFlow.answerCredentialPrompt(input);
  }

  function cancelSshCredentialPrompt() {
    return sshFlow.cancelCredentialPrompt();
  }

  function reconnectSerialAutoBaud(connectionId = connectionIdForSession(activeConnection.value)) {
    const connection = getConnection(connectionId);
    if (
      !connection ||
      !capabilitiesCan(
        sessionRegistry.getCapabilities(
          findSessionInstancesByConnectionId?.(connectionId)?.[0]?.id || "",
        ),
        "serialBaudDetection",
      )
    )
      return false;
    if (connection.baudRate !== "auto") return false;
    const session = findSessionInstancesByConnectionId?.(connectionId)?.[0] || null;
    const sessionId = session?.id || "";
    const terminalSessionId = session?.sessionId || "";
    if (!sessionId || !terminalSessionId || connectionRuntime.isPending(sessionId)) return false;
    activeSessions.value = new Set([...activeSessions.value, sessionId]);
    addSessionToTabOrder?.(sessionId);
    activeConnection.value = sessionId;
    activeTab.value = "shell";
    redetectSerialAutoBaud(sessionId).catch((error) => {
      logger.error("Failed to redetect serial auto baud session", error);
      const details = formatConnectionError(error);
      dispatchConnectionEvent(sessionId, {
        type: CONNECTION_EVENT.SERIAL_REDETECT_FAILED,
        payload: { errorMessage: details.detail || details.message || String(error) },
      });
      lastSerialBaudEvent.value = {
        connectionId,
        sessionId,
        baudRate: 0,
        confirmed: false,
        serialPort: "",
        scores: [],
        error: details.detail || details.message || String(error),
      };
    });
    return true;
  }

  async function redetectSerialAutoBaud(sessionId) {
    const connectionId = connectionIdForSession(sessionId);
    const terminalSessionId = getSessionInstance(sessionId)?.sessionId || "";
    if (!sessionId || !terminalSessionId) {
      throw new Error("Serial session is not active.");
    }
    logger.info("serial_auto_baud.redetect.requested", {
      connectionId,
      sessionId,
    });
    dispatchConnectionEvent(sessionId, {
      type: CONNECTION_EVENT.SERIAL_REDETECT_REQUESTED,
    });
    const response = await redetectBackendSerialBaud(terminalSessionId);
    dispatchConnectionEvent(sessionId, {
      type: CONNECTION_EVENT.SERIAL_REDETECT_SUCCEEDED,
      payload: response,
    });
    lastSerialBaudEvent.value = {
      connectionId,
      sessionId,
      baudRate: response?.baudRate || 0,
      confirmed: response?.confirmed === true,
      serialPort: response?.serialPort || "",
      scores: Array.isArray(response?.serialScores) ? response.serialScores : [],
      error: "",
    };
  }

  function toggleActiveSessionHighlight(
    connectionId = connectionIdForSession(activeConnection.value),
  ) {
    const connection = getConnection(connectionId);
    if (!connection) {
      logger.warn("terminal_highlight.toggle.missing", {
        connectionId,
      });
      return false;
    }
    if (connection.external) {
      logger.warn("terminal_highlight.toggle.external_ignored", {
        connectionId,
      });
      return false;
    }
    const enabled = connection.terminalHighlightEnabled === false;
    logger.info("terminal_highlight.toggle.requested", {
      connectionId,
      enabled,
    });
    updateConnection(connectionId, { terminalHighlightEnabled: enabled }).catch((error) => {
      logger.error("Failed to update terminal highlight setting", error);
    });
    return true;
  }

  return {
    addConnection,
    answerHostKeyPrompt,
    answerSshCredentialPrompt,
    cancelSshCredentialPrompt,
    closeSession,
    connectTo,
    handleBackendSessionEnded,
    reconnectSerialAutoBaud,
    removeConnection,
    selectConnection,
    toggleActiveSessionHighlight,
    updateConnection,
  };
}
