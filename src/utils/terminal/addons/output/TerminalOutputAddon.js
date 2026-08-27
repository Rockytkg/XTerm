const scheduleFrame =
  globalThis.requestAnimationFrame?.bind(globalThis) ?? ((callback) => setTimeout(callback, 16));
const cancelFrame =
  globalThis.cancelAnimationFrame?.bind(globalThis) ?? ((handle) => clearTimeout(handle));
const hasLineBreak = (value) => value.includes("\n");
const DEFAULT_WRITE_CHUNK_CHARS = 32 * 1024;
const DEFAULT_HIGH_WATERMARK = 10;
const DEFAULT_LOW_WATERMARK = 5;
const DEFAULT_FRAME_BUDGET_MS = 16;
const MIN_FLUSH_DELAY_MS = 4;
const MIN_WRITE_CHUNK_CHARS = 4096;
const FRAME_BUDGET_FRACTION = 0.72;
const WRITE_CALLBACK_TIMEOUT_MS = 250;

function nextFrame() {
  return new Promise((resolve) => scheduleFrame(resolve));
}

function nowMs() {
  return globalThis.performance?.now?.() ?? Date.now();
}

function clampWriteEnd(value, start, requestedEnd) {
  let resolved = Math.min(requestedEnd, value.length);
  if (resolved <= start || resolved >= value.length) return resolved;
  const previous = value.charCodeAt(resolved - 1);
  if (previous >= 0xd800 && previous <= 0xdbff) {
    resolved -= 1;
  }
  return Math.max(start + 1, resolved);
}

class DeferredGate {
  constructor(open = true) {
    this._open = open;
    this._waiters = [];
  }

  get open() {
    return this._open;
  }

  setOpen(open) {
    if (this._open === open) return;
    this._open = open;
    if (!open) return;
    const waiters = this._waiters.splice(0);
    for (const resolve of waiters) resolve();
  }

  wait() {
    if (this._open) return Promise.resolve();
    return new Promise((resolve) => {
      this._waiters.push(resolve);
    });
  }

  clear() {
    this._open = true;
    const waiters = this._waiters.splice(0);
    for (const resolve of waiters) resolve();
  }
}

class TerminalOutputFlowControl {
  constructor({
    highWatermark = DEFAULT_HIGH_WATERMARK,
    lowWatermark = DEFAULT_LOW_WATERMARK,
    onResume = null,
  } = {}) {
    this._highWatermark = Math.max(1, Number(highWatermark) || DEFAULT_HIGH_WATERMARK);
    this._lowWatermark = Math.max(
      0,
      Math.min(this._highWatermark - 1, Number(lowWatermark) || DEFAULT_LOW_WATERMARK),
    );
    this._onResume = onResume;
    this._pendingCallbacks = 0;
    this._drainWaiters = [];
    this._blocked = new DeferredGate(true);
    this._generation = 0;
  }

  async write(terminal, data) {
    if (!terminal || !data) return;
    await this._blocked.wait();
    const generation = this._generation;
    this._pendingCallbacks += 1;
    if (this._pendingCallbacks >= this._highWatermark) {
      this._blocked.setOpen(false);
    }
    let settled = false;
    let timeout;
    const finish = () => {
      if (settled) return;
      settled = true;
      if (timeout) clearTimeout(timeout);
      if (generation !== this._generation) return;
      this._pendingCallbacks = Math.max(0, this._pendingCallbacks - 1);
      if (!this._blocked.open && this._pendingCallbacks <= this._lowWatermark) {
        this._blocked.setOpen(true);
        // 低水位恢复 = 渲染端已经追上，通知上层上报 renderedOffset。
        this._onResume?.();
      }
      this._resolveDrained();
    };
    timeout = setTimeout(finish, WRITE_CALLBACK_TIMEOUT_MS);
    terminal.write(data, finish);
  }

  drain() {
    if (this._pendingCallbacks <= 0) return Promise.resolve();
    return new Promise((resolve) => {
      this._drainWaiters.push(resolve);
    });
  }

  reset() {
    this._generation += 1;
    this._pendingCallbacks = 0;
    this._blocked.clear();
    this._resolveDrained();
  }

  _resolveDrained() {
    if (this._pendingCallbacks > 0 || this._drainWaiters.length === 0) return;
    const waiters = this._drainWaiters.splice(0);
    for (const resolve of waiters) resolve();
  }
}

export class TerminalOutputAddon {
  constructor({
    onFlushComplete,
    onRecordChunk,
    onBackpressureResume,
    isDisposed,
    isRecordingActive,
    flushDelay,
    getFrameBudgetMs,
    maxChars,
    writeChunkChars,
    highWatermark = DEFAULT_HIGH_WATERMARK,
    lowWatermark = DEFAULT_LOW_WATERMARK,
  }) {
    this._onFlushComplete = onFlushComplete;
    this._onRecordChunk = onRecordChunk;
    this._hasRecording = typeof onRecordChunk === "function";
    this._isDisposed = isDisposed;
    this._isRecordingActive = isRecordingActive;
    this._flushDelay = flushDelay;
    this._getFrameBudgetMs = getFrameBudgetMs;
    this._maxChars = maxChars;
    this._terminal = null;
    this._segments = [];
    this._buffer = "";
    this._frame = undefined;
    this._timer = undefined;
    this._writeLock = Promise.resolve();
    this._flushWaiters = [];
    this._generation = 0;
    this._writeChunkChars = Math.max(
      MIN_WRITE_CHUNK_CHARS,
      Number(writeChunkChars) || DEFAULT_WRITE_CHUNK_CHARS,
    );
    this._flowControl = new TerminalOutputFlowControl({
      highWatermark,
      lowWatermark,
      onResume: onBackpressureResume,
    });
  }

  activate(terminal) {
    this._terminal = terminal;
  }

  dispose() {
    this.drop();
    this._terminal = null;
  }

  queue(data, { recordable = false, immediate = false } = {}) {
    if (!data) return;
    // 无记录回调时 _consumeRecordable 永远提前返回，push 只会让 _segments 无界增长。
    if (this._hasRecording) {
      this._segments.push({
        data,
        offset: 0,
        recordable: recordable && this._isRecordingEnabled(),
      });
    }
    this._buffer += data;
    if (immediate || this._buffer.length >= this._maxChars) {
      this.flush();
      return;
    }
    if (hasLineBreak(data)) {
      this._scheduleFrame();
      return;
    }
    this._schedule();
  }

  flush = () => {
    this._clearSchedule();
    if (!this._terminal || !this._buffer) {
      this._resolveFlushWaiters();
      return;
    }

    const output = this._buffer;
    this._buffer = "";
    const recordable = this._consumeRecordable(output.length);
    if (recordable && this._isRecordingEnabled()) {
      this._onRecordChunk(recordable);
    }

    const generation = this._generation;
    this._writeLock = this._writeLock
      .catch(() => {})
      .then(() => this._writeOutput(output, generation));
    this._writeLock.finally(() => this._resolveFlushWaiters()).catch(() => {});
  };

  waitForFlush() {
    if (!this._buffer && this._flushWaiters.length === 0) {
      return this._writeLock;
    }
    return new Promise((resolve) => {
      this._flushWaiters.push(resolve);
      this.flush();
    });
  }

  drop() {
    this._clearSchedule();
    this._generation += 1;
    this._segments = [];
    this._buffer = "";
    this._flowControl.reset();
    this._resolveFlushWaiters();
  }

  _schedule() {
    if (this._frame || this._timer) return;
    const flushDelay = this._resolveFlushDelay();
    this._timer = setTimeout(() => {
      this._timer = undefined;
      this._frame = scheduleFrame(this.flush);
    }, flushDelay);
  }

  _scheduleFrame() {
    if (this._frame) return;
    if (this._timer) {
      clearTimeout(this._timer);
      this._timer = undefined;
    }
    this._frame = scheduleFrame(this.flush);
  }

  _clearSchedule() {
    if (this._timer) {
      clearTimeout(this._timer);
      this._timer = undefined;
    }
    if (this._frame) {
      cancelFrame(this._frame);
      this._frame = undefined;
    }
  }

  async _writeOutput(output, generation) {
    if (!this._terminal || !output || this._isDisposed()) return;
    const viewport = this._captureViewportState();
    let offset = 0;
    let frameStartedAt = nowMs();
    while (
      generation === this._generation &&
      offset < output.length &&
      this._terminal &&
      !this._isDisposed()
    ) {
      const end = clampWriteEnd(output, offset, offset + this._writeChunkChars);
      await this._flowControl.write(this._terminal, output.slice(offset, end));
      offset = end;
      this._restoreViewportState(viewport);
      if (offset < output.length && this._shouldYieldFrame(frameStartedAt)) {
        await nextFrame();
        frameStartedAt = nowMs();
      }
    }
    if (generation !== this._generation) return;
    await this._flowControl.drain();
    if (generation !== this._generation) return;
    this._onFlushComplete?.(output);
  }

  _captureViewportState() {
    const buffer = this._terminal?.buffer?.active;
    if (!buffer) return null;
    return {
      atBottom: buffer.viewportY >= buffer.baseY,
      viewportY: buffer.viewportY,
    };
  }

  _restoreViewportState(viewport) {
    if (!viewport || viewport.atBottom || !this._terminal) return;
    this._terminal.scrollToLine(viewport.viewportY);
  }

  _shouldYieldFrame(frameStartedAt) {
    const frameBudget = Number(this._getFrameBudgetMs?.());
    const target = Number.isFinite(frameBudget) ? frameBudget : DEFAULT_FRAME_BUDGET_MS;
    return nowMs() - frameStartedAt >= Math.max(MIN_FLUSH_DELAY_MS, target * FRAME_BUDGET_FRACTION);
  }

  _resolveFlushDelay() {
    const frameBudget = Number(this._getFrameBudgetMs?.());
    const target = Number.isFinite(frameBudget) ? frameBudget : DEFAULT_FRAME_BUDGET_MS;
    return Math.max(MIN_FLUSH_DELAY_MS, Math.min(this._flushDelay, Math.round(target)));
  }

  _resolveFlushWaiters() {
    if (this._buffer || this._flushWaiters.length === 0) return;
    const waiters = this._flushWaiters.splice(0);
    for (const resolve of waiters) resolve(this._writeLock);
  }

  _isRecordingEnabled() {
    return this._hasRecording && this._isRecordingActive?.() === true;
  }

  _consumeRecordable(length) {
    if (!this._hasRecording || length <= 0) return "";
    let remaining = length;
    let recordable = "";
    while (remaining > 0 && this._segments.length > 0) {
      const segment = this._segments[0];
      const available = segment.data.length - segment.offset;
      const take = Math.min(available, remaining);
      if (segment.recordable) {
        recordable += segment.data.slice(segment.offset, segment.offset + take);
      }
      segment.offset += take;
      remaining -= take;
      if (segment.offset >= segment.data.length) {
        this._segments.shift();
      }
    }
    return recordable;
  }
}
