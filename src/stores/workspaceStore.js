import { computed, onScopeDispose, ref, watch } from "vue";
import { defineStore } from "pinia";
import { loadWorkspaceBootstrap, reorderConnectionProfiles } from "../services/workspace";
import { closeBackendSession } from "../services/terminalSessions";
import { isConnectionNotActiveError } from "../services/ipc/errors";
import { useAppPreferences } from "../composables/useAppPreferences";
import { createConnectionRuntime } from "./connectionRuntime";
import { createHostKeyPromptController } from "./hostKeyPromptController";
import { createSshCredentialPromptController } from "./sshCredentialPromptController";
import { createWorkspaceRecordingController } from "./workspaceRecordingController";
import { createWorkspaceRuntimeMetricsController } from "./workspaceRuntimeMetricsController";
import { createWorkspaceSessionRegistry } from "./workspaceSessionRegistry";
import { startWorkspaceEventListeners } from "./workspaceEvents";
import { createWorkspaceSessionController } from "./workspaceSessionController";
import { createTerminalPresentationGate } from "./terminalPresentationGate";
import { connectionRuntimeStatus as deriveConnectionRuntimeStatus } from "./connectionStateMachine";

import { createWorkspaceProxyModule } from "./workspaceProxyModule";
import { createWorkspaceFileServiceModule } from "./workspaceFileServiceModule";
import { createWorkspaceConnectionCatalog } from "./workspaceConnectionCatalog";
import { createWorkspaceExternalSessionController } from "./workspaceExternalSessions";
import { createLogger } from "../utils/logger";
import { createRuntimeId } from "../utils/runtimeIds";
import { i18n } from "../i18n";
import { showToast } from "../composables/useToasts";

const logger = createLogger("frontend.workspace.store");

function notifyInitStepFailed(name, error) {
  // 初始化失败此前只写日志，用户看到的是半初始化界面；这里补一条 toast。
  showToast({
    id: `workspace-init-${name}`,
    type: "error",
    title: i18n.global.t("notifications.workspaceInitFailed"),
    message: `${name}: ${error?.message || String(error)}`,
    duration: 6000,
  });
}

export const useWorkspaceStore = defineStore("workspace", () => {
  const connectionCatalog = createWorkspaceConnectionCatalog();
  const {
    connections,
    connectionsById,
    getConnection,
    patchRecord: patchConnection,
    profileConnections: connectionProfiles,
    removeExternal: removeExternalConnection,
    removeProfile,
    reorderProfiles,
    setProfiles,
    upsertExternal,
  } = connectionCatalog;
  const activeConnection = ref("");
  const activeTab = ref("shell");
  const activeSessions = ref(new Set());
  const sessionInstances = ref(new Map());
  const sessionTabOrder = ref([]);
  const lastSerialBaudEvent = ref(null);
  const activeTerminalSize = ref({ cols: 0, rows: 0 });
  const connectionRuntime = createConnectionRuntime();
  const hostKeyPromptController = createHostKeyPromptController();
  const { hostKeyPrompt } = hostKeyPromptController;
  const sshCredentialPromptController = createSshCredentialPromptController({
    getConnection,
  });
  const { sshCredentialPrompt } = sshCredentialPromptController;
  const { preferences, isDark, resolvedTheme, resetPreferences, toggleTheme } = useAppPreferences();
  const sessionRegistry = createWorkspaceSessionRegistry({
    onRetireBackendSession: (backendSessionId) => {
      closeBackendSession(backendSessionId).catch((error) => {
        // 已断开/已终结的后端会话属于预期失败，按错误码静默容错。
        if (isConnectionNotActiveError(error)) return;
        logger.warn("session.retire.close_failed", {
          backendSessionId,
          error: error?.message || String(error),
        });
      });
    },
  });
  const terminalPresentationGate = createTerminalPresentationGate();
  const proxyConfig = ref({ bindIp: "0.0.0.0", port: 3128, running: false });
  const proxyStats = ref({
    bindIp: "0.0.0.0",
    port: 3128,
    running: false,
    uploadBytesTotal: 0,
    downloadBytesTotal: 0,
    uploadBytesPerSec: 0,
    downloadBytesPerSec: 0,
  });
  const proxyInterfaces = ref([]);
  const fileServiceConfig = ref({
    protocol: "tftp",
    bindIp: "0.0.0.0",
    port: 69,
    sharedDir: "",
    username: "admin",
    // 口令不明文持有：只记录后端是否已配置（新快照契约的 passwordSet）。
    passwordSet: false,
    running: false,
  });
  const fileServiceTransfers = ref([]);

  const {
    hydrateProxy,
    refreshProxyInterfaces,
    startObserving: observeProxy,
    startProxyServer,
    stopProxyServer,
    updateProxyServerBindIp,
    updateProxyServerPort,
  } = createWorkspaceProxyModule({ proxyConfig, proxyStats, proxyInterfaces });
  const {
    clearFileTransfers,
    hydrateFileService,
    startObserving: observeFileService,
    startFileServiceServer,
    stopFileServiceServer,
    updateFileServiceBindIp,
    updateFileServiceSharedDir,
    updateFileServiceUsername,
    updateFileServicePassword,
  } = createWorkspaceFileServiceModule({ fileServiceConfig, fileServiceTransfers });
  let initializationPromise = null;
  let disposed = false;
  const runtimeDisposers = [];

  async function hydrateWorkspace() {
    logger.info("hydrate.start");
    try {
      const data = await loadWorkspaceBootstrap();
      setProfiles(data.connections);
      activeSessions.value = new Set();
      sessionInstances.value = new Map();
      sessionTabOrder.value = normalizeSessionTabOrder([...activeSessions.value]);
      activeConnection.value = "";
      logger.info("hydrate.success", {
        connectionCount: connections.value.length,
        activeSessionCount: activeSessions.value.size,
        activeConnectionId: activeConnection.value || null,
      });
    } catch (error) {
      logger.error("hydrate.failed", error);
      notifyInitStepFailed("hydrate_workspace", error);
    }
  }

  async function refreshConnectionList() {
    logger.debug("connections.refresh.start", {
      previousCount: connections.value.length,
    });
    try {
      const data = await loadWorkspaceBootstrap();
      setProfiles(data.connections);
      logger.info("connections.refresh.success", {
        connectionCount: connections.value.length,
      });
    } catch (error) {
      logger.error("connections.refresh.failed", error);
    }
  }

  function normalizeConnectionOrder(order) {
    const existing = new Set(connectionProfiles.value.map((connection) => connection.id));
    const seen = new Set();
    const normalized = [];

    for (const id of Array.isArray(order) ? order : []) {
      if (!existing.has(id) || seen.has(id)) continue;
      seen.add(id);
      normalized.push(id);
    }

    for (const connection of connectionProfiles.value) {
      if (seen.has(connection.id)) continue;
      seen.add(connection.id);
      normalized.push(connection.id);
    }

    return normalized;
  }

  function applyConnectionOrder(order) {
    reorderProfiles(order);
  }

  async function reorderConnections(order) {
    const normalizedOrder = normalizeConnectionOrder(order);
    if (
      sameOrder(
        normalizedOrder,
        connectionProfiles.value.map((connection) => connection.id),
      )
    )
      return false;

    const previousProfiles = connectionProfiles.value;
    logger.info("connections.reorder.requested", {
      previousOrder: previousProfiles.map((connection) => connection.id),
      nextOrder: normalizedOrder,
    });
    applyConnectionOrder(normalizedOrder);
    try {
      await reorderConnectionProfiles(normalizedOrder);
      await refreshConnectionList();
      logger.info("connections.reorder.success", {
        count: normalizedOrder.length,
      });
      return true;
    } catch (error) {
      setProfiles(previousProfiles);
      logger.error("connections.reorder.failed", error);
      throw error;
    }
  }

  function normalizeSessionTabOrder(order) {
    const activeIds = new Set(activeSessions.value);
    const seen = new Set();
    const normalized = [];

    for (const id of Array.isArray(order) ? order : []) {
      if (!activeIds.has(id) || !sessionInstances.value.has(id) || seen.has(id)) continue;
      seen.add(id);
      normalized.push(id);
    }

    for (const id of activeIds) {
      if (!sessionInstances.value.has(id) || seen.has(id)) continue;
      seen.add(id);
      normalized.push(id);
    }

    return normalized;
  }

  function sameOrder(a, b) {
    return a.length === b.length && a.every((id, index) => id === b[index]);
  }

  function setSessionTabOrder(order) {
    const normalizedOrder = normalizeSessionTabOrder(order);
    if (!sameOrder(normalizedOrder, sessionTabOrder.value)) {
      sessionTabOrder.value = normalizedOrder;
    }
  }

  function addSessionToTabOrder(connectionId) {
    if (!connectionId) return;
    setSessionTabOrder([...sessionTabOrder.value, connectionId]);
  }

  function removeSessionFromTabOrder(connectionId) {
    setSessionTabOrder(sessionTabOrder.value.filter((id) => id !== connectionId));
  }

  function reorderSessionTabs(order) {
    setSessionTabOrder(order);
  }

  const workspaceEvents = startWorkspaceEventListeners({
    dispatchConnectionEvent: sessionRegistry.dispatchConnectionEvent,
    hostKeyPromptController,
    onBackendSessionEnded: (frontendSessionId, backendSessionId) => {
      const session = getSessionInstance(frontendSessionId);
      if (session?.sessionId === backendSessionId) {
        clearSessionInstanceTerminalSession(frontendSessionId);
      }
      handleBackendSessionEnded(frontendSessionId);
    },
    sessionRegistry,
  });

  async function runInitStep(name, operation) {
    try {
      return await operation();
    } catch (error) {
      logger.error(`init.${name}.failed`, error);
      notifyInitStepFailed(name, error);
      return null;
    }
  }

  async function initializeWorkspace() {
    logger.info("init.start");

    // Observe before hydrating. Config events advance each module's revision,
    // preventing a slower bootstrap response from overwriting newer state.
    const observationResults = await Promise.all([
      runInitStep("events", () => workspaceEvents.start()),
      runInitStep("proxy_events", observeProxy),
      runInitStep("file_service_events", observeFileService),
    ]);
    const observationDisposers = observationResults.filter(
      (dispose) => typeof dispose === "function",
    );
    if (disposed) {
      observationDisposers.forEach((dispose) => dispose());
    } else {
      runtimeDisposers.push(...observationDisposers);
    }

    await Promise.all([
      runInitStep("hydrate_workspace", hydrateWorkspace),
      runInitStep("hydrate_proxy", hydrateProxy),
      runInitStep("hydrate_file_service", hydrateFileService),
    ]);
    await runInitStep("deeplinks", () => externalSessions.startDeepLinks());
    logger.info("init.complete");
  }

  function init() {
    if (!initializationPromise) {
      initializationPromise = initializeWorkspace();
    }
    return initializationPromise;
  }

  onScopeDispose(() => {
    disposed = true;
    terminalPresentationGate.dispose();
    workspaceEvents.stop();
    externalSessions.stopDeepLinks();
    while (runtimeDisposers.length > 0) runtimeDisposers.pop()?.();
  });

  function connectionRuntimeStatus(connectionId, fallbackStatus = "offline") {
    const stateStatus = sessionRegistry.getConnectionState(connectionId)?.status;
    return deriveConnectionRuntimeStatus(stateStatus, fallbackStatus);
  }

  function createSessionInstance(connectionId, options = {}) {
    if (!connectionId) return null;
    const existingId = options.id || options.sessionId || `terminal-${createRuntimeId()}`;
    const existing = existingId ? sessionInstances.value.get(existingId) : null;
    if (existing) return existing;
    const id = existingId;
    const next = {
      id,
      connectionId,
      createdAt: Date.now(),
      sessionId: options.terminalSessionId || options.sessionId || "",
      terminalKey: options.terminalKey || id,
    };
    sessionInstances.value = new Map(sessionInstances.value).set(id, next);
    return next;
  }

  function createPendingSessionInstance(connectionId, options = {}) {
    const id = options.id || `terminal-${createRuntimeId()}`;
    return createSessionInstance(connectionId, {
      id,
      terminalSessionId: "",
      terminalKey: id,
    });
  }

  function bindSessionInstanceToTerminalSession(frontendSessionId, terminalSessionId) {
    if (!frontendSessionId || !terminalSessionId) return null;
    const existing = sessionInstances.value.get(frontendSessionId);
    if (!existing) return null;
    const next = {
      ...existing,
      sessionId: terminalSessionId,
      terminalKey: existing.terminalKey || existing.id,
    };
    sessionInstances.value = new Map(sessionInstances.value).set(frontendSessionId, next);
    return next;
  }

  function clearSessionInstanceTerminalSession(sessionId) {
    if (!sessionId || !sessionInstances.value.has(sessionId)) return null;
    const existing = sessionInstances.value.get(sessionId);
    const next = { ...existing, sessionId: "" };
    sessionInstances.value = new Map(sessionInstances.value).set(sessionId, next);
    return next;
  }

  function removeSessionInstance(sessionId) {
    if (!sessionId || !sessionInstances.value.has(sessionId)) return;
    const next = new Map(sessionInstances.value);
    next.delete(sessionId);
    sessionInstances.value = next;
  }

  function getSessionInstance(sessionId) {
    return sessionInstances.value.get(sessionId) || null;
  }

  function findSessionInstancesByConnectionId(connectionId) {
    return [...sessionInstances.value.values()].filter(
      (session) => session.connectionId === connectionId,
    );
  }

  /**
   * Stable session list — returns connection objects WITHOUT runtime fields.
   * Use `sessionRuntime(connectionId)` to access reactive runtime state.
   * This separation means connection-state changes don't force rebuilding N
   * session objects; only the specific runtime accessor re-evaluates.
   */
  const openSessions = computed(() =>
    normalizeSessionTabOrder(sessionTabOrder.value)
      .map((id) => {
        const session = sessionInstances.value.get(id);
        const connection = session ? connectionsById.value.get(session.connectionId) : null;
        if (!session || !connection) return null;
        return {
          ...connection,
          id: session.id,
          sessionId: session.sessionId || "",
          connectionId: session.connectionId,
          terminalKey: session.terminalKey || session.id,
        };
      })
      .filter(Boolean),
  );

  /**
   * Reactive runtime fields for a single connection.
   * Templates call `sessionRuntime(session.id)` to get status/state without
   * baking those into every openSessions object.
   */
  function sessionRuntime(sessionId) {
    return {
      get connectionState() {
        return sessionRegistry.getConnectionState(sessionId);
      },
      get capabilities() {
        return sessionRegistry.getCapabilities(sessionId);
      },
      get status() {
        return connectionRuntimeStatus(sessionId);
      },
    };
  }

  const activeConnectionInfo = computed(() => {
    const id = activeConnection.value;
    if (!id || !activeSessions.value.has(id)) return null;
    const session = sessionInstances.value.get(id);
    const connection = session ? connectionsById.value.get(session.connectionId) : null;
    if (!session || !connection) return null;
    return {
      ...connection,
      id: session.id,
      sessionId: session.sessionId || "",
      connectionId: session.connectionId,
      terminalKey: session.terminalKey || session.id,
      ...sessionRuntime(id),
    };
  });

  const activeRuntimeMetrics = computed(() =>
    activeConnection.value ? sessionRegistry.getRuntimeMetrics(activeConnection.value) : null,
  );

  const activeSessionId = computed(() => activeConnection.value || "");

  const activeRemoteWorkingDirectory = computed(() =>
    sessionRegistry.getWorkingDirectoryByConnection(activeConnection.value),
  );

  const activeConnectionState = computed(() =>
    activeConnection.value
      ? sessionRegistry.getConnectionState(activeConnection.value)
      : sessionRegistry.getConnectionState(""),
  );

  const { forgetRuntimeMetricsSession } = createWorkspaceRuntimeMetricsController({
    activeConnection,
    activeConnectionInfo,
    getActiveSession: () => activeConnectionInfo.value?.sessionId || "",
    getActiveBackendChannel: () =>
      activeConnection.value
        ? sessionRegistry.getActiveSessionChannelId(activeConnection.value)
        : null,
    getConnectionProtocol: (sessionId) =>
      connectionsById.value.get(sessionInstances.value.get(sessionId)?.connectionId)?.protocol,
  });

  const { recordTerminalChunk, sessionRecordings, toggleSessionRecording, closeSessionRecording } =
    createWorkspaceRecordingController();

  const sessionController = createWorkspaceSessionController({
    activeTerminalSize,
    activeConnection,
    activeSessions,
    activeTab,
    connectionRuntime,
    getConnection,
    getSessionInstance,
    createSessionInstance,
    createPendingSessionInstance,
    bindSessionInstanceToTerminalSession,
    clearSessionInstanceTerminalSession,
    findSessionInstancesByConnectionId,
    removeSessionInstance,
    hostKeyPromptController,
    sshCredentialPromptController,
    lastSerialBaudEvent,
    preferences: computed(() => preferences),
    patchConnection,
    refreshConnectionList,
    addSessionToTabOrder,
    removeSessionFromTabOrder,
    removeExternalConnection,
    removeProfile,
    sessionRegistry,
    upsertExternalConnection: upsertExternal,
    dispatchConnectionEvent: sessionRegistry.dispatchConnectionEvent,
    closeSessionRecording,
    forgetRuntimeMetricsSession,
    waitForTerminalPresentation: terminalPresentationGate.wait,
    cancelTerminalPresentationWait: terminalPresentationGate.cancel,
  });
  const {
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
  } = sessionController;

  const externalSessions = createWorkspaceExternalSessionController({
    connectTo,
    upsertExternalConnection: upsertExternal,
  });

  watch([connections, activeSessions], () => {
    setSessionTabOrder(sessionTabOrder.value);
  });

  watch(activeConnection, (next, previous) => {
    if (next === previous) return;
    logger.info("active_connection.changed", {
      previousConnectionId: previous || null,
      nextConnectionId: next || null,
    });
  });

  watch(activeTab, (next, previous) => {
    if (next === previous) return;
    logger.debug("active_tab.changed", {
      previousTab: previous,
      nextTab: next,
    });
  });

  function selectTab(tabId) {
    logger.info("tab.select", {
      previousTab: activeTab.value,
      nextTab: tabId,
    });
    activeTab.value = tabId;
  }

  return {
    activeTerminalSize,
    activeConnection,
    activeSessionId,
    activeConnectionInfo,
    activeConnectionState,
    activeRuntimeMetrics,
    activeRemoteWorkingDirectory,
    activeTab,
    addConnection,
    closeSession,
    connectTo,
    connectionProfiles,
    connections,
    hostKeyPrompt,
    init,
    isDark,
    lastSerialBaudEvent,
    openSessions,
    sessionRuntime,
    preferences,
    resolvedTheme,
    proxyConfig,
    proxyInterfaces,
    proxyStats,
    refreshProxyInterfaces,
    fileServiceConfig,
    fileServiceTransfers,
    recordTerminalChunk,
    reconnectSerialAutoBaud,
    reorderConnections,
    refreshConnectionList,
    removeConnection,
    resetPreferences,
    reorderSessionTabs,
    selectConnection,
    selectTab,
    sessionRegistry,
    markTerminalPresentationReady: terminalPresentationGate.ready,
    sessionRecordings,
    sshCredentialPrompt,
    toggleActiveSessionHighlight,
    toggleSessionRecording,
    toggleTheme,
    updateConnection,
    startProxyServer,
    stopProxyServer,
    updateProxyServerBindIp,
    updateProxyServerPort,
    clearFileTransfers,
    startFileServiceServer,
    stopFileServiceServer,
    updateFileServiceBindIp,
    updateFileServiceSharedDir,
    updateFileServiceUsername,
    updateFileServicePassword,
    answerHostKeyPrompt,
    answerSshCredentialPrompt,
    cancelSshCredentialPrompt,
  };
});
