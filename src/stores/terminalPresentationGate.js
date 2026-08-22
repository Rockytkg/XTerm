const DEFAULT_PRESENTATION_TIMEOUT_MS = 3_000;

export function createTerminalPresentationGate({
  timeoutMs = DEFAULT_PRESENTATION_TIMEOUT_MS,
} = {}) {
  const waiters = new Map();

  function finish(sessionId, result) {
    const waiter = waiters.get(sessionId);
    if (!waiter) return false;
    waiters.delete(sessionId);
    clearTimeout(waiter.timer);
    waiter.resolve(result);
    return true;
  }

  function wait(sessionId) {
    if (!sessionId) return Promise.resolve("invalid");
    finish(sessionId, "superseded");
    return new Promise((resolve) => {
      const timer = setTimeout(() => finish(sessionId, "timeout"), timeoutMs);
      waiters.set(sessionId, { resolve, timer });
    });
  }

  function ready(sessionId) {
    return finish(sessionId, "ready");
  }

  function cancel(sessionId) {
    return finish(sessionId, "cancelled");
  }

  function dispose() {
    for (const sessionId of [...waiters.keys()]) cancel(sessionId);
  }

  return { cancel, dispose, ready, wait };
}
