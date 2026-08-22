import { onScopeDispose, watch } from "vue";
import { stopSshRuntimeMetrics, startSshRuntimeMetrics } from "../services/terminalSessions";
import { formatConnectionError } from "./workspaceUtils";
import { createLogger } from "../utils/logger";
import { connectionCan } from "../utils/connectionCapabilities";

const logger = createLogger("frontend.workspace.runtime_metrics");

const MAX_PENDING_STOPS = 32;

export function createWorkspaceRuntimeMetricsController({
  activeConnection,
  activeConnectionInfo,
  getActiveBackendChannel,
  getActiveSession,
}) {
  let activeMetricsSession = null;
  let pendingStartTimer = undefined;
  const pendingStops = new Map();
  const METRICS_START_SETTLE_MS = 250;

  function metricsStopKey(connectionId, sessionId) {
    return `${connectionId}:${sessionId}`;
  }

  function isExpectedMetricsStopError(error) {
    const normalized = formatConnectionError(error);
    const code = String(normalized?.code || "").toUpperCase();
    const detail = String(normalized?.detail || normalized?.message || "");
    return code === "INVALID_REQUEST" && detail.includes("connection is not active");
  }

  function stopRuntimeMetricsSession(
    connectionId,
    sessionId,
    failureMessage = "runtime_metrics.stop.failed",
  ) {
    if (!connectionId || !sessionId) {
      return Promise.resolve(false);
    }
    // Guard against unbounded Map growth during rapid session switches
    if (
      !pendingStops.has(metricsStopKey(connectionId, sessionId)) &&
      pendingStops.size >= MAX_PENDING_STOPS
    ) {
      const oldest = pendingStops.keys().next().value;
      pendingStops.delete(oldest);
    }

    const key = metricsStopKey(connectionId, sessionId);
    const pending = pendingStops.get(key);
    if (pending) return pending;

    const stopPromise = stopSshRuntimeMetrics(connectionId, sessionId)
      .then(() => true)
      .catch((error) => {
        if (isExpectedMetricsStopError(error)) {
          logger.debug("runtime_metrics.stop.ignored", {
            connectionId,
            sessionId,
            reason: formatConnectionError(error).detail || "connection is not active",
          });
          return false;
        }
        logger.error(failureMessage, error);
        return false;
      })
      .finally(() => {
        pendingStops.delete(key);
      });

    pendingStops.set(key, stopPromise);
    return stopPromise;
  }

  function forgetRuntimeMetricsSession(connectionId, sessionId) {
    clearPendingStart();
    if (
      activeMetricsSession?.connectionId === connectionId &&
      activeMetricsSession?.sessionId === sessionId
    ) {
      activeMetricsSession = null;
    }
    pendingStops.delete(metricsStopKey(connectionId, sessionId));
  }

  function clearPendingStart() {
    clearTimeout(pendingStartTimer);
    pendingStartTimer = undefined;
  }

  function stopActiveMetricsSession({ backendOwnsCleanup = false } = {}) {
    clearPendingStart();
    if (!activeMetricsSession) return;
    const { connectionId, sessionId } = activeMetricsSession;
    activeMetricsSession = null;
    if (backendOwnsCleanup) return;
    void stopRuntimeMetricsSession(connectionId, sessionId, "runtime_metrics.stop.failed");
  }

  function syncActiveMetricsSession() {
    const connection = activeConnectionInfo.value;
    const sessionId = getActiveSession();
    const activeChannelId = getActiveBackendChannel?.();
    const shouldMonitor = connectionCan(connection, "metrics") && !!sessionId && !!activeChannelId;

    if (
      shouldMonitor &&
      activeMetricsSession?.connectionId === connection.connectionId &&
      activeMetricsSession?.sessionId === sessionId
    ) {
      return;
    }

    stopActiveMetricsSession();
    if (!shouldMonitor) {
      clearPendingStart();
      return;
    }

    clearPendingStart();
    pendingStartTimer = setTimeout(() => {
      pendingStartTimer = undefined;
      if (
        activeConnectionInfo.value?.sessionId !== connection.sessionId ||
        getActiveSession() !== sessionId ||
        !getActiveBackendChannel?.()
      ) {
        return;
      }
      activeMetricsSession = { connectionId: connection.connectionId, sessionId };
      startSshRuntimeMetrics(connection.connectionId, sessionId).catch((error) => {
        logger.error("runtime_metrics.start.failed", error);
        if (
          activeMetricsSession?.connectionId === connection.connectionId &&
          activeMetricsSession?.sessionId === sessionId
        ) {
          activeMetricsSession = null;
        }
      });
    }, METRICS_START_SETTLE_MS);
  }

  watch(
    [
      () => activeConnection.value,
      () => getActiveSession(),
      () => getActiveBackendChannel?.(),
      () => connectionCan(activeConnectionInfo.value, "metrics"),
    ],
    syncActiveMetricsSession,
  );

  onScopeDispose(() => {
    stopActiveMetricsSession();
    pendingStops.clear();
  });

  return {
    forgetRuntimeMetricsSession,
  };
}
