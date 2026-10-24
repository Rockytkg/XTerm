export const TERMINAL_EVENTS = Object.freeze({
  SFTP_TRANSFER_PROGRESS: "sftp.transfer_progress",
});

export const sftpTransfersTestState = {
  listeners: new Set(),
  observeCalls: 0,
  rafFlushes: new Set(),
  toasts: [],
};

export function resetSftpTransfersTestState() {
  sftpTransfersTestState.listeners.clear();
  sftpTransfersTestState.observeCalls = 0;
  sftpTransfersTestState.rafFlushes.clear();
  sftpTransfersTestState.toasts.length = 0;
}

export async function observeTerminalEvent(_type, handler) {
  sftpTransfersTestState.observeCalls += 1;
  sftpTransfersTestState.listeners.add(handler);
  return () => sftpTransfersTestState.listeners.delete(handler);
}

export function emitSftpTransferProgress(payload) {
  for (const listener of [...sftpTransfersTestState.listeners]) listener(payload);
}

export function flushSftpTransferFrames() {
  for (const flush of [...sftpTransfersTestState.rafFlushes]) flush();
}

export function showToast(options) {
  sftpTransfersTestState.toasts.push(options);
  return sftpTransfersTestState.toasts.length;
}

export function createRafThrottle(callback) {
  let frame = false;
  let lastArgs = [];

  function schedule(...args) {
    lastArgs = args;
    if (frame) return;
    frame = true;
    sftpTransfersTestState.rafFlushes.add(schedule.flush);
  }

  schedule.cancel = () => {
    frame = false;
    sftpTransfersTestState.rafFlushes.delete(schedule.flush);
  };
  schedule.flush = () => {
    if (!frame) return;
    frame = false;
    sftpTransfersTestState.rafFlushes.delete(schedule.flush);
    callback(...lastArgs);
  };
  return schedule;
}

export function createDebounced(callback) {
  let lastArgs = null;

  function schedule(...args) {
    lastArgs = args;
  }

  schedule.cancel = () => {
    lastArgs = null;
  };
  schedule.flush = () => {
    if (!lastArgs) return;
    callback(...lastArgs);
    lastArgs = null;
  };
  return schedule;
}
