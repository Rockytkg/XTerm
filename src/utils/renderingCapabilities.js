const DEFAULT_FRAME_INTERVAL_MS = 16.7;
const MIN_FRAME_INTERVAL_MS = 4;
const MAX_FRAME_INTERVAL_MS = 33;
const SAMPLE_TARGET = 90;

function clampFrameInterval(value) {
  if (!Number.isFinite(value) || value <= 0) return DEFAULT_FRAME_INTERVAL_MS;
  return Math.min(MAX_FRAME_INTERVAL_MS, Math.max(MIN_FRAME_INTERVAL_MS, value));
}

export function createFrameIntervalSampler({ sampleTarget = SAMPLE_TARGET } = {}) {
  let frameIntervalMs = DEFAULT_FRAME_INTERVAL_MS;
  let previousTimestamp = 0;
  let samples = 0;
  let frame = 0;
  let running = false;

  function sample(timestamp) {
    if (!running) return;
    if (previousTimestamp > 0) {
      const delta = clampFrameInterval(timestamp - previousTimestamp);
      frameIntervalMs = samples === 0 ? delta : frameIntervalMs * 0.82 + delta * 0.18;
      samples += 1;
    }
    previousTimestamp = timestamp;
    if (samples < sampleTarget) {
      frame = window.requestAnimationFrame(sample);
    } else {
      frame = 0;
      running = false;
    }
  }

  function start() {
    if (running || typeof window === "undefined" || !window.requestAnimationFrame) return;
    running = true;
    previousTimestamp = 0;
    samples = 0;
    frame = window.requestAnimationFrame(sample);
  }

  function stop() {
    running = false;
    if (frame && typeof window !== "undefined") {
      window.cancelAnimationFrame(frame);
    }
    frame = 0;
  }

  function currentFrameIntervalMs() {
    return clampFrameInterval(frameIntervalMs);
  }

  return {
    currentFrameIntervalMs,
    start,
    stop,
  };
}
