import { createLogger } from "../../../logger.js";

const logger = createLogger("frontend.terminal.output-addon");

const DEFAULT_WRITE_CHUNK_CHARS = 32 * 1024;
const MIN_WRITE_CHUNK_CHARS = 4096;

function clampWriteEnd(value, start, requestedEnd) {
  let resolved = Math.min(requestedEnd, value.length);
  if (resolved <= start || resolved >= value.length) return resolved;
  const previous = value.charCodeAt(resolved - 1);
  if (previous >= 0xd800 && previous <= 0xdbff) {
    resolved -= 1;
  }
  return Math.max(start + 1, resolved);
}

export class TerminalOutputAddon {
  constructor({ onRecordChunk, isDisposed, isRecordingActive, writeChunkChars }) {
    this._onRecordChunk = onRecordChunk;
    this._hasRecording = typeof onRecordChunk === "function";
    this._isDisposed = isDisposed;
    this._isRecordingActive = isRecordingActive;
    this._terminal = null;
    this._segments = [];
    this._segmentHead = 0;
    this._buffer = "";
    this._writeLock = Promise.resolve();
    this._flushWaiters = [];
    this._generation = 0;
    this._pendingWriteResolve = null;
    this._writeChunkChars = Math.max(
      MIN_WRITE_CHUNK_CHARS,
      Number(writeChunkChars) || DEFAULT_WRITE_CHUNK_CHARS,
    );
  }

  activate(terminal) {
    this._terminal = terminal;
  }

  dispose() {
    this.drop();
    this._terminal = null;
  }

  queue(data, { recordable = false } = {}) {
    if (!data) return;
    // 有录制消费者时必须为整批输出保留边界，才能在同一批内筛出可录制内容；
    // 录制未激活时入队的段 recordable 恒为 false，直接跳过可以避免每个输出包
    // 都产生一次分段分配。
    if (this._hasRecording && this._isRecordingEnabled()) {
      this._segments.push({
        data,
        offset: 0,
        recordable,
      });
    }
    this._buffer += data;
    // 直写：数据到达立即进入 xterm 写入队列。帧对齐与主线程让出由库内建机制
    // 负责——RenderDebouncer 每个 RAF 最多渲染一次，WriteBuffer 按 12ms 预算
    // 切片解析；在数据通路再做合帧/水位/限时只是增加延迟的负优化。
    this.flush();
  }

  flush = () => {
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
    this._writeLock
      .finally(() => this._resolveFlushWaiters())
      .catch((error) => logger.error("terminal-output.write.failed", error));
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
    this._generation += 1;
    this._segments = [];
    this._segmentHead = 0;
    this._buffer = "";
    // 终端销毁时 xterm 会静默丢弃队列中的 write 回调；主动收束在途等待，
    // 避免 writeLock 永远挂起。
    this._pendingWriteResolve?.();
    this._pendingWriteResolve = null;
    this._resolveFlushWaiters();
  }

  async _writeOutput(output, generation) {
    if (!this._terminal || !output || this._isDisposed()) return;
    let offset = 0;
    let lastChunkParsed = null;
    // 整个冲刷是同步的（分片之间不 await），视口不可能在中途被移动，无需
    // 捕获/恢复。只等最后一片的解析回调：WriteBuffer 按 FIFO 顺序解析，它
    // 触发即代表本批全部解析完成。
    while (
      generation === this._generation &&
      offset < output.length &&
      this._terminal &&
      !this._isDisposed()
    ) {
      const end = clampWriteEnd(output, offset, offset + this._writeChunkChars);
      lastChunkParsed = new Promise((resolve) => {
        this._pendingWriteResolve = resolve;
        this._terminal.write(output.slice(offset, end), resolve);
      });
      offset = end;
    }
    try {
      if (lastChunkParsed) await lastChunkParsed;
    } finally {
      this._pendingWriteResolve = null;
    }
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
    while (remaining > 0 && this._segmentHead < this._segments.length) {
      const segment = this._segments[this._segmentHead];
      const available = segment.data.length - segment.offset;
      const take = Math.min(available, remaining);
      if (segment.recordable) {
        recordable += segment.data.slice(segment.offset, segment.offset + take);
      }
      segment.offset += take;
      remaining -= take;
      if (segment.offset >= segment.data.length) {
        this._segmentHead += 1;
      }
    }
    // 消费游标避免每个小输出包都触发 Array.shift() 的线性搬移；定期压缩保持
    // 活跃录制期间 segment 存储有界，且压缩的总成本为摊还 O(1)。
    if (this._segmentHead === this._segments.length) {
      this._segments = [];
      this._segmentHead = 0;
    } else if (this._segmentHead >= 1024 && this._segmentHead * 2 >= this._segments.length) {
      this._segments = this._segments.slice(this._segmentHead);
      this._segmentHead = 0;
    }
    return recordable;
  }
}
