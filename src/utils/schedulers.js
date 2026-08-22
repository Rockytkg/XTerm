export function createDebounced(callback, delayMs) {
  let timer = 0;
  let lastArgs = [];

  function cancel() {
    window.clearTimeout(timer);
    timer = 0;
  }

  function schedule(...args) {
    lastArgs = args;
    cancel();
    timer = window.setTimeout(() => {
      timer = 0;
      callback(...lastArgs);
    }, delayMs);
  }

  schedule.cancel = cancel;
  return schedule;
}

export function createRafThrottle(callback) {
  let frame = 0;
  let lastArgs = [];

  function cancel() {
    if (frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
    }
  }

  function schedule(...args) {
    lastArgs = args;
    if (frame) return;
    frame = window.requestAnimationFrame(() => {
      frame = 0;
      callback(...lastArgs);
    });
  }

  schedule.cancel = cancel;
  schedule.flush = () => {
    if (!frame) return;
    cancel();
    callback(...lastArgs);
  };
  return schedule;
}
