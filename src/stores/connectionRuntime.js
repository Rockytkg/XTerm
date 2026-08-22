import { createLogger } from "../utils/logger.js";

const logger = createLogger("frontend.workspace.connection_runtime");

const PHASE_IDLE = "idle";
const PHASE_OPENING = "opening";
const PHASE_CLOSING = "closing";

export function createConnectionRuntime() {
  const phases = new Map();
  let nextAttemptToken = 1;

  function phase(connectionId) {
    return phases.get(connectionId)?.phase || PHASE_IDLE;
  }

  function begin(connectionId) {
    if (phase(connectionId) === PHASE_CLOSING) {
      logger.warn("connection.begin.refused", {
        connectionId,
        reason: "close_in_progress",
      });
      return null;
    }
    const attemptToken = nextAttemptToken++;
    phases.set(connectionId, { phase: PHASE_OPENING, attemptToken });
    logger.info("connection.begin", { connectionId, attemptToken });
    return attemptToken;
  }

  function cancel(connectionId, attemptToken) {
    const current = phases.get(connectionId);
    if (attemptToken != null && current?.attemptToken !== attemptToken) return false;
    phases.set(connectionId, {
      phase: PHASE_CLOSING,
      attemptToken: current?.attemptToken ?? attemptToken ?? null,
    });
    logger.warn("connection.cancel", { connectionId, attemptToken: current?.attemptToken ?? null });
    return true;
  }

  function closeComplete(connectionId) {
    const previous = phase(connectionId);
    phases.delete(connectionId);
    if (previous !== PHASE_IDLE) {
      logger.info("connection.close_complete", { connectionId });
    }
  }

  function isCurrent(connectionId, attemptToken) {
    const current = phases.get(connectionId);
    return (
      current?.phase === PHASE_OPENING &&
      (attemptToken == null || current.attemptToken === attemptToken)
    );
  }

  function isPending(connectionId) {
    return phase(connectionId) === PHASE_OPENING;
  }

  function finish(connectionId, attemptToken) {
    const current = phases.get(connectionId);
    if (
      current?.phase !== PHASE_OPENING ||
      (attemptToken != null && current.attemptToken !== attemptToken)
    ) {
      logger.debug("connection.finish.ignored", { connectionId, phase: phase(connectionId) });
      return;
    }
    phases.delete(connectionId);
    logger.info("connection.finish", { connectionId, attemptToken: current.attemptToken });
  }

  return {
    begin,
    cancel,
    closeComplete,
    finish,
    isCurrent,
    isPending,
  };
}
