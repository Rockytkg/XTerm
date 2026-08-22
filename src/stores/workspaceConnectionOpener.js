import { closeBackendSession, openBackendConnection } from "../services/terminalSessions";
import { createLogger } from "../utils/logger";
import { formatConnectionError } from "./workspaceUtils";
import { CONNECTION_EVENT } from "./connectionStateMachine";
import { resolveTerminalGeometry } from "./terminalGeometry";
import { applyOpenResponseMetadata } from "./workspaceRuntimeSync";

const logger = createLogger("frontend.workspace.connection_opener");

export function createWorkspaceConnectionOpener({
  activeTerminalSize,
  connectionRuntime,
  dispatchConnectionEvent,
  getConnection,
  onSessionOpened,
  onOpenError,
  onOpenResponse,
  preferences,
  sessionRegistry,
}) {
  async function openConnectionInBackground(connectionId, options = {}) {
    const frontendSessionId = options.sessionId || connectionId;
    const openRequestId = options.openRequestId || frontendSessionId;
    const attemptToken = options.attemptToken;
    const requestLogger = logger.withContext({
      connectionId,
      frontendSessionId,
      openRequestId,
      module: "workspace.connection_opener",
    });
    const connection = getConnection?.(connectionId);
    // Per-connection overrides take priority; fall back to global preferences.
    const terminalType = connection?.terminalType || preferences.value.terminalType;
    const terminalScrollback =
      connection?.terminalScrollback ?? preferences.value.terminalScrollback;
    // Coerce empty string → undefined so the backend receives None rather than Some("").
    const encoding = connection?.encoding || undefined;
    // Detection ON when encoding is "auto" (falsy), OFF for a specific encoding.
    const realtimeEncodingDetection = connection?.realtimeEncodingDetection ?? !encoding;
    let keepOpenForAuthentication = false;

    requestLogger.info("connection.open.dispatch", {
      terminalType,
      encoding: encoding || null,
      realtimeEncodingDetection,
    });
    try {
      const response = await openBackendConnection(connectionId, {
        openRequestId,
        terminalScrollback,
        terminalType,
        encoding,
        realtimeEncodingDetection,
        sshCredential: options.sshCredential,
        ...resolveTerminalGeometry(activeTerminalSize),
      });
      if (!connectionRuntime.isCurrent(frontendSessionId, attemptToken)) {
        requestLogger.warn("connection.open.completed.stale");
        if (response?.sessionId) {
          closeBackendSession(response.sessionId).catch((error) => {
            requestLogger.error("connection.open.stale_session_close_failed", error);
          });
        }
        return;
      }
      if (response?.status === "connected" && response?.sessionId) {
        const sessionId = onSessionOpened?.(connectionId, response.sessionId, {
          response,
          attemptToken,
          sessionId: options.sessionId || "",
          preserveActiveTab: !!options.preserveActiveTab,
        });
        applyOpenResponseMetadata({
          dispatchConnectionEvent,
          response,
          sessionId: sessionId || response.sessionId,
          sessionRegistry,
        });
      }
      onOpenResponse?.(connectionId, response, {
        attemptToken,
        sessionId: options.sessionId || "",
      });
      keepOpenForAuthentication = response?.awaiting === "hostKeyChallenge";
      requestLogger.info("connection.open.completed");
    } catch (error) {
      requestLogger.error("connection.open.failed", error);
      if (
        onOpenError?.(connectionId, error, {
          attemptToken,
          sessionId: options.sessionId || "",
        })
      )
        return;
      if (connectionRuntime.isCurrent(frontendSessionId, attemptToken)) {
        if (frontendSessionId) {
          dispatchConnectionEvent(frontendSessionId, {
            type: CONNECTION_EVENT.OPEN_FAILED,
            payload: { error: formatConnectionError(error) },
          });
        }
      } else {
        requestLogger.warn("connection.open.failed.stale");
      }
    } finally {
      if (!keepOpenForAuthentication) {
        sessionRegistry.finishSessionAttempt(frontendSessionId, attemptToken);
        connectionRuntime.finish(frontendSessionId, attemptToken);
      }
    }
  }

  return {
    openConnectionInBackground,
  };
}
