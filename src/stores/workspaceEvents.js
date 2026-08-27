import { TERMINAL_EVENTS, observeTerminalEvent } from "../events/terminalEventBus";
import { createLogger, isLogLevelEnabled, summarizeValue } from "../utils/logger";
import { toFiniteOrNull } from "./workspaceUtils";
import { CONNECTION_EVENT, connectionEventForSessionStatus } from "./connectionStateMachine";

const logger = createLogger("frontend.workspace.events");

export function startWorkspaceEventListeners({
  dispatchConnectionEvent,
  hostKeyPromptController,
  onBackendSessionEnded,
  sessionRegistry,
}) {
  let started = false;
  let stopped = false;
  const unlistenFns = [];

  // start() 串行 await 期间 store 可能已 dispose（stop 先弹空数组）：
  // 之后到达的 unlisten 必须立即调用，否则订阅残留。
  function trackUnlisten(unlisten) {
    if (!stopped) {
      unlistenFns.push(unlisten);
      return;
    }
    try {
      unlisten?.();
    } catch (error) {
      logger.error("workspaceEvents: failed to unsubscribe:", error);
    }
  }

  function sessionAcceptsRuntimeUpdates(sessionId) {
    return !["closed", "failed"].includes(sessionRegistry.getConnectionState(sessionId).status);
  }

  function stop() {
    logger.info("listeners.stop", {
      listenerCount: unlistenFns.length,
    });
    while (unlistenFns.length > 0) {
      const unlisten = unlistenFns.pop();
      try {
        unlisten?.();
      } catch (error) {
        logger.error("workspaceEvents: failed to unsubscribe:", error);
      }
    }
    started = false;
    stopped = true;
  }

  async function start() {
    if (started) return;
    started = true;
    stopped = false;
    logger.info("listeners.start");

    try {
      trackUnlisten(
        await observeTerminalEvent(TERMINAL_EVENTS.CONNECTION_HOST_KEY_CHALLENGE, (payload) => {
          if (!payload?.connectionId) return;
          const frontendSessionId = sessionRegistry.getFrontendSessionIdForOpenRequest(
            payload.sessionId,
          );
          if (!frontendSessionId) return;
          logger.warn("connection.host_key_challenge", {
            connectionId: payload.connectionId,
            host: payload.host,
            port: payload.port,
            algorithm: payload.algorithm,
          });
          const accepted = hostKeyPromptController.setPrompt({
            ...payload,
            attemptToken: sessionRegistry.getAttemptToken(frontendSessionId),
            connectionId: payload.connectionId,
            openRequestId: payload.sessionId,
            sessionId: frontendSessionId,
          });
          if (!accepted) return;
          dispatchConnectionEvent(frontendSessionId, {
            type: CONNECTION_EVENT.HOST_KEY_CHALLENGE,
          });
        }),
      );

      trackUnlisten(
        await observeTerminalEvent(TERMINAL_EVENTS.SESSION_STATUS_CHANGED, (payload) => {
          if (!payload?.sessionId) return;
          const sessionId = payload.sessionId;
          const connectionId = payload.connectionId || sessionRegistry.getConnectionId(sessionId);
          if (!connectionId) return;
          const nextStatus = payload.state || "pending";
          const detail = payload.detail || "";
          // info 级别关闭时跳过 summarizeValue(payload) 的序列化开销。
          if (isLogLevelEnabled("info")) {
            logger.info("session.status.changed", {
              connectionId,
              sessionId,
              nextStatus,
              payload: summarizeValue(payload),
            });
          }
          const event = connectionEventForSessionStatus(nextStatus, detail);
          const result = event
            ? sessionRegistry.dispatchBackendConnectionEvent(sessionId, connectionId, event)
            : { routing: "ignored" };
          if (result.ended && result.frontendSessionId) {
            onBackendSessionEnded?.(result.frontendSessionId, sessionId);
          }
          logger.debug("session.status.routed", {
            connectionId,
            sessionId,
            routing: result.routing,
          });
        }),
      );

      trackUnlisten(
        await observeTerminalEvent(TERMINAL_EVENTS.SESSION_WORKING_DIRECTORY, (payload) => {
          if (!payload?.sessionId || !payload.path) return;
          const frontendSessionId = sessionRegistry.getFrontendSessionId(payload.sessionId);
          if (!frontendSessionId || !sessionAcceptsRuntimeUpdates(frontendSessionId)) return;
          sessionRegistry.setSessionWorkingDirectory(frontendSessionId, payload.path);
        }),
      );

      trackUnlisten(
        await observeTerminalEvent(TERMINAL_EVENTS.METRICS_REPORT, (payload) => {
          const sessionId = payload?.sessionId;
          if (!payload?.connectionId || !sessionId) return;
          const frontendSessionId = sessionRegistry.getFrontendSessionId(sessionId);
          if (!frontendSessionId) return;
          if (sessionRegistry.getConnectionId(frontendSessionId) !== payload.connectionId) return;
          if (!sessionAcceptsRuntimeUpdates(frontendSessionId)) return;
          const previousMetrics = sessionRegistry.getRuntimeMetrics(frontendSessionId) ?? {};
          const keepFinite = (next, previous) => {
            const value = toFiniteOrNull(next);
            return value ?? previous ?? null;
          };
          const metrics = payload.unavailable
            ? {
                unavailable: true,
                latencyMs: null,
                sampleTimestampMs: toFiniteOrNull(payload.sampleTimestampMs),
              }
            : {
                ...previousMetrics,
                unavailable: false,
                cpuReady: payload.cpuReady ?? true,
                cpuPercent: toFiniteOrNull(payload.cpuPercent),
                cpuUserPercent: toFiniteOrNull(payload.cpuUserPercent),
                cpuSystemPercent: toFiniteOrNull(payload.cpuSystemPercent),
                cpuIowaitPercent: toFiniteOrNull(payload.cpuIowaitPercent),
                cpuStealPercent: toFiniteOrNull(payload.cpuStealPercent),
                memoryPercent: toFiniteOrNull(payload.memoryPercent),
                diskPercent: toFiniteOrNull(payload.diskPercent),
                loadAverage: payload.loadAverage ?? "—",
                latencyMs: toFiniteOrNull(payload.latencyMs),
                memoryTotal: toFiniteOrNull(payload.memoryTotal),
                memoryUsed: toFiniteOrNull(payload.memoryUsed),
                memoryAvailable: toFiniteOrNull(payload.memoryAvailable),
                swapTotal: toFiniteOrNull(payload.swapTotal),
                swapUsed: toFiniteOrNull(payload.swapUsed),
                swapPercent: toFiniteOrNull(payload.swapPercent),
                diskTotal: toFiniteOrNull(payload.diskTotal),
                diskUsed: toFiniteOrNull(payload.diskUsed),
                diskAvailable: toFiniteOrNull(payload.diskAvailable),
                diskInodePercent: keepFinite(
                  payload.diskInodePercent,
                  previousMetrics.diskInodePercent,
                ),
                networkRxRate: toFiniteOrNull(payload.networkRxRate),
                networkTxRate: toFiniteOrNull(payload.networkTxRate),
                processCount: keepFinite(payload.processCount, previousMetrics.processCount),
                threadCount: keepFinite(payload.threadCount, previousMetrics.threadCount),
                uptimeSeconds: toFiniteOrNull(payload.uptimeSeconds),
                sampleTimestampMs: toFiniteOrNull(payload.sampleTimestampMs) ?? Date.now(),
              };
          sessionRegistry.setRuntimeMetrics(frontendSessionId, metrics);
        }),
      );
    } catch (error) {
      logger.error("listeners.start.failed", error);
      stop();
      throw error;
    }
  }

  return { start, stop };
}
