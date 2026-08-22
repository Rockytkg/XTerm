import {
  createTerminalSessionRuntimeController,
  TERMINAL_SESSION_RUNTIME_EVENTS,
} from "../utils/terminal/TerminalSessionRuntimeController";

export function useTerminalSessionRuntime({
  drainOutput,
  dropOutput,
  getContext,
  logger,
  onSessionData,
  queueResizeSync,
  setActiveSessionChannel,
  transport,
  writeStatus,
}) {
  const controller = createTerminalSessionRuntimeController({
    drainOutput,
    dropOutput,
    getContext,
    logger,
    queueResizeSync,
    setActiveSessionChannel,
    transport,
    writeStatus,
  });
  const stopSessionData = controller.on(
    TERMINAL_SESSION_RUNTIME_EVENTS.SESSION_DATA,
    onSessionData,
  );

  return {
    controller,
    dispose() {
      stopSessionData?.();
    },
  };
}
