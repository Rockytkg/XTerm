import { asBytes, bytesToBinaryString } from "./bytes";
import { TransferError } from "./errors";

function joinBytes(chunks, length) {
  const result = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

export function isVtSequenceEnd(byte) {
  return (byte >= 0x61 && byte <= 0x7a) || (byte >= 0x41 && byte <= 0x5a);
}

function isProtocolByte(byte) {
  return (
    (byte >= 0x61 && byte <= 0x7a) ||
    (byte >= 0x41 && byte <= 0x5a) ||
    (byte >= 0x30 && byte <= 0x39) ||
    byte === 0x23 ||
    byte === 0x3a ||
    byte === 0x2b ||
    byte === 0x2f ||
    byte === 0x3d
  );
}

export class TransferBuffer {
  constructor() {
    this._queue = [];
    this._pending = null;
    this._next = null;
    this._nextOffset = 0;
    this._scratch = new ArrayBuffer(128);
    this._stopped = false;
  }

  add(value) {
    if (this._stopped) return;
    this._queue.push(value);
    this._pending?.resolve?.();
    this._pending = null;
  }

  stop() {
    this._stopped = true;
    this._pending?.reject?.(new TransferError("Stopped"));
    this._pending = null;
  }

  drain() {
    this._queue = [];
    this._next = null;
    this._nextOffset = 0;
  }

  async _take() {
    if (this._next && this._nextOffset < this._next.length) {
      return this._next.subarray(this._nextOffset);
    }
    while (!this._queue.length) {
      if (this._stopped) throw new TransferError("Stopped");
      await new Promise((resolve, reject) => {
        this._pending = { resolve, reject };
      });
    }
    this._next = asBytes(this._queue.shift());
    this._nextOffset = 0;
    return this._next;
  }

  async readLine() {
    const chunks = [];
    let length = 0;
    while (true) {
      let next = await this._take();
      const newline = next.indexOf(0x0a);
      if (newline >= 0) {
        this._nextOffset += newline + 1;
        next = next.subarray(0, newline);
      } else {
        this._nextOffset += next.length;
      }
      if (next.includes(0x03)) throw new TransferError("Interrupted");
      chunks.push(next);
      length += next.length;
      if (newline >= 0) return bytesToBinaryString(joinBytes(chunks, length));
    }
  }

  async readWindowsLine() {
    let buffer = new Uint8Array(this._scratch);
    let lastByte = 0x1b;
    let skipEscape = false;
    let hasNewline = false;
    let mayDuplicate = false;
    let hasCursorHome = false;
    let previousHadCursorHome = false;
    let offset = 0;
    while (true) {
      let next = await this._take();
      const marker = next.indexOf(0x21);
      if (marker >= 0) {
        this._nextOffset += marker + 1;
        next = next.subarray(0, marker);
      } else {
        this._nextOffset += next.length;
      }
      for (const byte of next) {
        if (byte === 0x03) throw new TransferError("Interrupted");
        if (byte === 0x0a) hasNewline = true;
        if (skipEscape) {
          if (isVtSequenceEnd(byte)) {
            skipEscape = false;
            if (byte === 0x48 && lastByte >= 0x30 && lastByte <= 0x39) {
              mayDuplicate = true;
            }
          }
          if (lastByte === 0x5b && byte === 0x48) {
            hasCursorHome = true;
          }
          lastByte = byte;
        } else if (byte === 0x1b) {
          skipEscape = true;
          lastByte = byte;
        } else if (isProtocolByte(byte)) {
          if (mayDuplicate) {
            mayDuplicate = false;
            if (
              hasNewline &&
              offset > 0 &&
              (byte === buffer[offset - 1] || previousHadCursorHome)
            ) {
              buffer[offset - 1] = byte;
              continue;
            }
          }
          if (offset >= buffer.length) {
            const grown = new Uint8Array(buffer.length * 2 + next.length);
            grown.set(buffer.subarray(0, offset));
            buffer = grown;
            this._scratch = grown.buffer;
          }
          buffer[offset++] = byte;
          previousHadCursorHome = hasCursorHome;
          hasCursorHome = false;
          hasNewline = false;
        }
      }
      if (marker >= 0 && offset > 0 && !skipEscape) {
        return bytesToBinaryString(buffer.subarray(0, offset));
      }
    }
  }

  async readBinary(size) {
    const result = new Uint8Array(size);
    let offset = 0;
    while (offset < size) {
      let next = await this._take();
      const length = Math.min(size - offset, next.length);
      result.set(next.subarray(0, length), offset);
      offset += length;
      this._nextOffset += length;
    }
    return result;
  }
}
