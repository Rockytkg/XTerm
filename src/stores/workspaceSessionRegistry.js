import { shallowRef, triggerRef } from "vue";
import {
  CONNECTION_EVENT,
  IDLE_CONNECTION_STATE,
  reduceConnectionState,
} from "./connectionStateMachine.js";
import {
  EMPTY_CONNECTION_CAPABILITIES,
  normalizeConnectionCapabilities,
} from "../utils/connectionCapabilities.js";

const MAX_RETIRED_BACKEND_SESSIONS = 512;
const MAX_PENDING_BACKEND_SESSIONS = 128;

function hasStateChanges(previous, next) {
  return Object.keys(next).some((key) => !sameValue(previous[key], next[key]));
}

function sameValue(a, b) {
  if (Object.is(a, b)) return true;
  if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
  return a.every((value, index) => Object.is(value, b[index]));
}

function hasRuntimeMetricChanges(previous, next) {
  if (!previous) return true;
  const previousKeys = Object.keys(previous);
  const nextKeys = Object.keys(next);
  if (previousKeys.length !== nextKeys.length) return true;
  return nextKeys.some((key) => !Object.is(previous[key], next[key]));
}

export function createWorkspaceSessionRegistry({ onRetireBackendSession } = {}) {
  const records = shallowRef(new Map());
  const frontendSessionByBackendSession = new Map();
  const frontendSessionByOpenRequest = new Map();
  const pendingBackendEvents = new Map();
  const retiredBackendSessions = new Set();

  function emptyRecord(connectionId = "") {
    return {
      activeChannelId: null,
      attemptToken: null,
      backendSessionId: "",
      capabilities: EMPTY_CONNECTION_CAPABILITIES,
      connectionId,
      connectionState: IDLE_CONNECTION_STATE,
      openRequestId: "",
      runtimeMetrics: null,
      workingDirectory: "",
    };
  }

  function recordFor(frontendSessionId) {
    return frontendSessionId ? records.value.get(frontendSessionId) : undefined;
  }

  function ensureRecord(frontendSessionId, connectionId = "") {
    if (!frontendSessionId) return null;
    const existing = records.value.get(frontendSessionId);
    if (existing) return existing;
    const next = emptyRecord(connectionId);
    records.value.set(frontendSessionId, next);
    triggerRef(records);
    return next;
  }

  function patchRecord(frontendSessionId, patch) {
    const previous = ensureRecord(frontendSessionId, patch.connectionId || "");
    if (!previous) return null;
    const next = { ...previous, ...patch };
    if (Object.keys(patch).every((key) => Object.is(previous[key], next[key]))) return previous;
    records.value.set(frontendSessionId, next);
    triggerRef(records);
    return next;
  }

  function rememberRetiredBackendSession(backendSessionId) {
    if (!backendSessionId) return;
    retiredBackendSessions.delete(backendSessionId);
    retiredBackendSessions.add(backendSessionId);
    while (retiredBackendSessions.size > MAX_RETIRED_BACKEND_SESSIONS) {
      retiredBackendSessions.delete(retiredBackendSessions.values().next().value);
    }
    pendingBackendEvents.delete(backendSessionId);
  }

  function unbindBackendSession(frontendSessionId, { retire = true, releaseBackend = true } = {}) {
    const backendSessionId = recordFor(frontendSessionId)?.backendSessionId || "";
    if (backendSessionId) {
      frontendSessionByBackendSession.delete(backendSessionId);
      if (retire) {
        rememberRetiredBackendSession(backendSessionId);
        // force-reconnect 等路径只改前端映射，旧后端会话会孤儿驻留；
        // 由注入的回调负责 closeBackendSession（失败按错误码静默容错）。
        if (releaseBackend) onRetireBackendSession?.(backendSessionId);
      }
    }
    patchRecord(frontendSessionId, {
      activeChannelId: null,
      backendSessionId: "",
      capabilities: EMPTY_CONNECTION_CAPABILITIES,
      runtimeMetrics: null,
      workingDirectory: "",
    });
    return backendSessionId;
  }

  function dispatchConnectionEvent(frontendSessionId, event) {
    if (!frontendSessionId) return false;
    const previous = getConnectionState(frontendSessionId);
    const next = reduceConnectionState(previous, event);
    if (!hasStateChanges(previous, next)) return false;
    patchRecord(frontendSessionId, { connectionState: next });
    return true;
  }

  function applyBackendEvent(frontendSessionId, event) {
    dispatchConnectionEvent(frontendSessionId, event);
    const ended = [CONNECTION_EVENT.SESSION_CLOSED, CONNECTION_EVENT.SESSION_FAILED].includes(
      event.type,
    );
    // 会话已由后端终结，无需再释放（避免一次必然失败的 close IPC）。
    if (ended) unbindBackendSession(frontendSessionId, { releaseBackend: false });
    return ended;
  }

  function beginSessionAttempt(frontendSessionId, connectionId, attemptToken, openRequestId) {
    if (!frontendSessionId || !connectionId || attemptToken == null || !openRequestId) return false;
    const previousOpenRequestId = recordFor(frontendSessionId)?.openRequestId || "";
    if (previousOpenRequestId) frontendSessionByOpenRequest.delete(previousOpenRequestId);
    unbindBackendSession(frontendSessionId);
    frontendSessionByOpenRequest.set(openRequestId, frontendSessionId);
    patchRecord(frontendSessionId, {
      attemptToken,
      connectionId,
      openRequestId,
    });
    dispatchConnectionEvent(frontendSessionId, { type: CONNECTION_EVENT.OPEN_REQUESTED });
    return true;
  }

  function bindBackendSession(frontendSessionId, backendSessionId, attemptToken) {
    const record = recordFor(frontendSessionId);
    if (
      !record ||
      !backendSessionId ||
      record.attemptToken !== attemptToken ||
      retiredBackendSessions.has(backendSessionId)
    ) {
      return false;
    }

    if (record.backendSessionId && record.backendSessionId !== backendSessionId) {
      unbindBackendSession(frontendSessionId);
    }
    retiredBackendSessions.delete(backendSessionId);
    if (record.openRequestId) frontendSessionByOpenRequest.delete(record.openRequestId);
    frontendSessionByBackendSession.set(backendSessionId, frontendSessionId);
    patchRecord(frontendSessionId, { backendSessionId, openRequestId: "" });

    const pending = pendingBackendEvents.get(backendSessionId) || [];
    pendingBackendEvents.delete(backendSessionId);
    for (const entry of pending) {
      if (!entry.connectionId || entry.connectionId === record.connectionId) {
        applyBackendEvent(frontendSessionId, entry.event);
      }
    }
    return true;
  }

  function bufferBackendEvent(backendSessionId, connectionId, event) {
    const pending = pendingBackendEvents.get(backendSessionId) || [];
    pending.push({ connectionId, event });
    pendingBackendEvents.set(backendSessionId, pending.slice(-4));
    while (pendingBackendEvents.size > MAX_PENDING_BACKEND_SESSIONS) {
      pendingBackendEvents.delete(pendingBackendEvents.keys().next().value);
    }
  }

  function dispatchBackendConnectionEvent(backendSessionId, connectionId, event) {
    if (!backendSessionId || !event) return { routing: "invalid" };
    const frontendSessionId = frontendSessionByBackendSession.get(backendSessionId);
    if (frontendSessionId) {
      const record = recordFor(frontendSessionId);
      if (connectionId && record?.connectionId !== connectionId) {
        return { routing: "mismatch" };
      }
      const ended = applyBackendEvent(frontendSessionId, event);
      return { ended, frontendSessionId, routing: "applied" };
    }
    if (retiredBackendSessions.has(backendSessionId)) return { routing: "stale" };
    bufferBackendEvent(backendSessionId, connectionId, event);
    return { routing: "buffered" };
  }

  function getConnectionId(frontendSessionId) {
    return recordFor(frontendSessionId)?.connectionId ?? "";
  }

  function getFrontendSessionId(backendSessionId) {
    return frontendSessionByBackendSession.get(backendSessionId) || "";
  }

  function getFrontendSessionIdForOpenRequest(openRequestId) {
    return frontendSessionByOpenRequest.get(openRequestId) || "";
  }

  function getBackendSessionId(frontendSessionId) {
    return recordFor(frontendSessionId)?.backendSessionId || "";
  }

  function getAttemptToken(frontendSessionId) {
    return recordFor(frontendSessionId)?.attemptToken ?? null;
  }

  function getOpenRequestId(frontendSessionId) {
    return recordFor(frontendSessionId)?.openRequestId || "";
  }

  function finishSessionAttempt(frontendSessionId, attemptToken) {
    const record = recordFor(frontendSessionId);
    if (!record || record.attemptToken !== attemptToken || record.backendSessionId) return false;
    if (record.openRequestId) frontendSessionByOpenRequest.delete(record.openRequestId);
    patchRecord(frontendSessionId, { openRequestId: "" });
    return true;
  }

  function getConnectionState(frontendSessionId) {
    return recordFor(frontendSessionId)?.connectionState ?? IDLE_CONNECTION_STATE;
  }

  function getCapabilities(frontendSessionId) {
    return recordFor(frontendSessionId)?.capabilities ?? EMPTY_CONNECTION_CAPABILITIES;
  }

  function getActiveSessionChannelId(frontendSessionId) {
    return recordFor(frontendSessionId)?.activeChannelId ?? null;
  }

  function getRuntimeMetrics(frontendSessionId) {
    return recordFor(frontendSessionId)?.runtimeMetrics ?? null;
  }

  function getWorkingDirectoryByConnection(frontendSessionId) {
    return recordFor(frontendSessionId)?.workingDirectory || "";
  }

  function setActiveSessionChannel(frontendSessionId, channelId) {
    if (!frontendSessionId) return;
    const nextChannelId = Number(channelId);
    const previous = getActiveSessionChannelId(frontendSessionId);
    const normalized = Number.isFinite(nextChannelId) ? nextChannelId : null;
    if (previous === normalized) return;
    patchRecord(frontendSessionId, { activeChannelId: normalized });
  }

  function setConnectionCapabilities(frontendSessionId, capabilities) {
    if (!frontendSessionId) return;
    patchRecord(frontendSessionId, {
      capabilities: normalizeConnectionCapabilities(capabilities),
    });
  }

  function setSessionWorkingDirectory(frontendSessionId, path) {
    if (!frontendSessionId || getWorkingDirectoryByConnection(frontendSessionId) === path) return;
    patchRecord(frontendSessionId, { workingDirectory: path || "" });
  }

  function clearConnectionRuntime(frontendSessionId) {
    if (!frontendSessionId) return;
    const openRequestId = recordFor(frontendSessionId)?.openRequestId || "";
    if (openRequestId) frontendSessionByOpenRequest.delete(openRequestId);
    unbindBackendSession(frontendSessionId);
    records.value.delete(frontendSessionId);
    triggerRef(records);
  }

  function setRuntimeMetrics(frontendSessionId, metrics) {
    if (!hasRuntimeMetricChanges(getRuntimeMetrics(frontendSessionId), metrics)) return;
    patchRecord(frontendSessionId, { runtimeMetrics: metrics });
  }

  return {
    beginSessionAttempt,
    bindBackendSession,
    clearConnectionRuntime,
    dispatchBackendConnectionEvent,
    dispatchConnectionEvent,
    finishSessionAttempt,
    getActiveSessionChannelId,
    getAttemptToken,
    getBackendSessionId,
    getCapabilities,
    getConnectionId,
    getConnectionState,
    getFrontendSessionId,
    getFrontendSessionIdForOpenRequest,
    getOpenRequestId,
    getRuntimeMetrics,
    getWorkingDirectoryByConnection,
    setActiveSessionChannel,
    setConnectionCapabilities,
    setRuntimeMetrics,
    setSessionWorkingDirectory,
    unbindBackendSession,
  };
}
