import { bytesEqual, bytesToUtf8, decodeBuffer, encodeBuffer } from "./bytes";
import { DEFAULT_MAX_CHUNK_SIZE, EMPTY_MD5, TRZSZ_VERSION } from "./constants";
import { normalizeError, protocolParseError, TransferError } from "./errors";
import { escapeCodes, escapeData, unescapeData } from "./escape";
import { TransferBuffer } from "./terminalBuffer";
import { stripTmuxStatusLine } from "./text";

export class TrzszTransfer {
  constructor({ sendToServer, maxDataChunkSize = DEFAULT_MAX_CHUNK_SIZE }) {
    this._sendToServer = sendToServer;
    this._buffer = new TransferBuffer();
    this._maxDataChunkSize = maxDataChunkSize;
    this._protocolNewline = "\n";
    this._remoteIsWindows = false;
    this._tmuxOutputJunk = false;
    this._config = {};
    this._opened = [];
    this._stopped = false;
    this._lastInputTime = 0;
    this._cleanTimeout = 100;
    this._maxChunkTime = 0;
  }

  addReceivedData(data) {
    if (!this._stopped) this._buffer.add(data);
    this._lastInputTime = Date.now();
  }

  async cleanup() {
    await Promise.allSettled(this._opened.map((file) => file.closeFile?.()));
    this._opened = [];
  }

  stopTransferring() {
    this._cleanTimeout = Math.max(this._maxChunkTime * 2, 500);
    this._stopped = true;
    this._buffer.stop();
  }

  async _cleanInput(timeout) {
    this._stopped = true;
    this._buffer.drain();
    this._lastInputTime = Date.now();
    while (timeout - (Date.now() - this._lastInputTime) > 0) {
      await new Promise((resolve) =>
        setTimeout(resolve, timeout - (Date.now() - this._lastInputTime)),
      );
    }
  }

  _sendLine(type, value) {
    this._sendToServer(`#${type}:${value}${this._protocolNewline}`);
  }

  async _recvLine(expectType, mayHaveJunk = false) {
    if (this._stopped) throw new TransferError("Stopped");
    let line = this._remoteIsWindows
      ? await this._buffer.readWindowsLine()
      : await this._buffer.readLine();
    if (this._remoteIsWindows || this._tmuxOutputJunk || mayHaveJunk) {
      if (!this._remoteIsWindows && line.length > 0) {
        while (line.endsWith("\r")) {
          line = line.substring(0, line.length - 1) + (await this._buffer.readLine());
        }
      }
      const expected = line.lastIndexOf(`#${expectType}:`);
      if (expected >= 0) {
        line = line.substring(expected);
      } else {
        const hash = line.lastIndexOf("#");
        if (hash > 0) line = line.substring(hash);
      }
      line = stripTmuxStatusLine(line);
    }
    return line;
  }

  async _recvCheck(expectType, mayHaveJunk = false) {
    const line = await this._recvLine(expectType, mayHaveJunk);
    const colon = line.indexOf(":");
    if (colon < 1) throw protocolParseError(line);
    const type = line.substring(1, colon);
    const value = line.substring(colon + 1);
    if (type !== expectType) throw new TransferError(value, type, true);
    return value;
  }

  _sendInteger(type, value) {
    this._sendLine(type, String(value));
  }

  async _recvInteger(type, mayHaveJunk = false) {
    return Number(await this._recvCheck(type, mayHaveJunk));
  }

  async _checkInteger(expected) {
    const value = await this._recvInteger("SUCC");
    if (value !== expected)
      throw new TransferError(`Integer check [${value}] <> [${expected}]`, null, true);
  }

  _sendString(type, value) {
    this._sendLine(type, encodeBuffer(value));
  }

  async _recvString(type, mayHaveJunk = false) {
    return bytesToUtf8(decodeBuffer(await this._recvCheck(type, mayHaveJunk)));
  }

  _sendBinary(type, value) {
    this._sendLine(type, encodeBuffer(value));
  }

  async _recvBinary(type, mayHaveJunk = false) {
    return decodeBuffer(await this._recvCheck(type, mayHaveJunk));
  }

  async _checkBinary(expected) {
    const value = await this._recvBinary("SUCC");
    if (!bytesEqual(value, expected)) throw new TransferError("Binary check failed", null, true);
  }

  _sendData(data, binary, codes) {
    if (!binary) {
      this._sendBinary("DATA", data);
      return;
    }
    const escaped = escapeData(data, codes);
    this._sendToServer(`#DATA:${escaped.length}\n`);
    this._sendToServer(escaped);
  }

  async _recvData(binary, codes, timeout) {
    let timer;
    try {
      return await Promise.race([
        new Promise((_, reject) => {
          timer = setTimeout(() => {
            this._cleanTimeout = 3000;
            reject(new TransferError("Receive data timeout"));
          }, timeout);
        }),
        (async () => {
          if (!binary) return this._recvBinary("DATA");
          const size = await this._recvInteger("DATA");
          return unescapeData(await this._buffer.readBinary(size), codes);
        })(),
      ]);
    } finally {
      clearTimeout(timer);
    }
  }

  sendAction(confirm, remoteIsWindows) {
    const action = {
      lang: "js",
      confirm,
      version: TRZSZ_VERSION,
      support_dir: true,
    };
    if (remoteIsWindows) {
      action.binary = false;
      action.newline = "!\n";
      this._remoteIsWindows = true;
      this._protocolNewline = "!\n";
    }
    this._sendString("ACT", JSON.stringify(action));
  }

  async recvConfig() {
    const config = JSON.parse(await this._recvString("CFG", true));
    this._config = config;
    this._tmuxOutputJunk = config.tmux_output_junk === true;
    return config;
  }

  clientExit(message) {
    this._sendString("EXIT", message);
  }

  async clientError(error) {
    await this._cleanInput(this._cleanTimeout);
    const normalized = normalizeError(error);
    const message = TransferError.message(normalized);
    let trace = true;
    if (normalized instanceof TransferError) {
      trace = normalized.isTraceBack();
      if (normalized.isRemoteExit()) return;
      if (normalized.isRemoteFail()) return;
    }
    this._sendString(trace ? "FAIL" : "fail", message);
  }

  async sendFiles(files, progress) {
    this._opened.push(...files);
    const binary = this._config.binary === true;
    const directory = this._config.directory === true;
    const maxChunkSize = this._config.bufsize
      ? Math.min(this._config.bufsize, this._maxDataChunkSize)
      : this._maxDataChunkSize;
    const codes = escapeCodes(this._config.escape_chars);
    this._sendInteger("NUM", files.length);
    await this._checkInteger(files.length);
    progress?.onNum?.(files.length);
    const names = [];
    for (const file of files) {
      const remoteName = await this._sendFile(file, {
        directory,
        binary,
        codes,
        maxChunkSize,
        progress,
      });
      if (!names.includes(remoteName)) names.push(remoteName);
    }
    return names;
  }

  async _sendFile(file, { directory, binary, codes, maxChunkSize, progress }) {
    const relPath = file.getRelPath();
    const fileName = relPath[relPath.length - 1];
    if (directory) {
      this._sendString(
        "NAME",
        JSON.stringify({
          path_id: file.getPathId(),
          path_name: relPath,
          is_dir: file.isDir(),
        }),
      );
    } else {
      this._sendString("NAME", fileName);
    }
    const remoteName = await this._recvString("SUCC");
    progress?.onName?.(fileName);
    if (file.isDir()) return remoteName;
    const size = file.getSize();
    this._sendInteger("SIZE", size);
    await this._checkInteger(size);
    progress?.onSize?.(size);
    const digest = await this._sendFileData(file, size, binary, codes, maxChunkSize, progress);
    await file.closeFile?.();
    this._sendBinary("MD5", digest);
    await this._checkBinary(digest);
    progress?.onDone?.();
    return remoteName;
  }

  async _sendFileData(file, size, binary, codes, maxChunkSize, progress) {
    let transferred = 0;
    let chunkSize = 1024;
    let buffer = new ArrayBuffer(chunkSize);
    progress?.onStep?.(0);
    while (transferred < size) {
      const started = Date.now();
      const data = await file.readFile(buffer);
      if (!data.length)
        throw new TransferError(`Read ${file.getRelPath().join("/")} returned no data`);
      this._sendData(data, binary, codes);
      await file.consumeDigest?.(data);
      await this._checkInteger(data.length);
      transferred += data.length;
      progress?.onStep?.(transferred);
      const elapsed = Date.now() - started;
      if (data.length === chunkSize && elapsed < 500 && chunkSize < maxChunkSize) {
        chunkSize = Math.min(chunkSize * 2, maxChunkSize);
        buffer = new ArrayBuffer(chunkSize);
      } else if (elapsed >= 2000 && chunkSize > 1024) {
        chunkSize = 1024;
        buffer = new ArrayBuffer(chunkSize);
      }
      this._maxChunkTime = Math.max(this._maxChunkTime, elapsed);
    }
    return file.finishDigest ? await file.finishDigest() : EMPTY_MD5;
  }

  async recvFiles(saveParam, progress) {
    const binary = this._config.binary === true;
    const directory = this._config.directory === true;
    const overwrite = this._config.overwrite === true;
    const timeout = this._config.timeout ? this._config.timeout * 1000 : 100000;
    const codes = escapeCodes(this._config.escape_chars);
    const count = await this._recvInteger("NUM");
    this._sendInteger("SUCC", count);
    progress?.onNum?.(count);
    const names = [];
    for (let index = 0; index < count; index += 1) {
      const localName = await this._recvFile(saveParam, {
        directory,
        overwrite,
        binary,
        codes,
        timeout,
        progress,
      });
      if (!names.includes(localName)) names.push(localName);
    }
    return names;
  }

  async _recvFile(saveParam, { directory, overwrite, binary, codes, timeout, progress }) {
    const encodedName = await this._recvString("NAME");
    const file = await saveParam.openSaveFile(saveParam, encodedName, directory, overwrite);
    this._sendString("SUCC", file.getLocalName());
    progress?.onName?.(file.getFileName());
    if (file.isDir()) return file.getLocalName();
    this._opened.push(file);
    const size = await this._recvInteger("SIZE");
    this._sendInteger("SUCC", size);
    progress?.onSize?.(size);
    await this._recvFileData(file, size, binary, codes, timeout, progress);
    const digest = await file.getDigest();
    await file.closeFile?.();
    const expected = await this._recvBinary("MD5");
    if (!bytesEqual(digest, expected)) throw new TransferError("Check MD5 failed");
    this._sendBinary("SUCC", digest);
    progress?.onDone?.();
    return file.getLocalName();
  }

  async _recvFileData(file, size, binary, codes, timeout, progress) {
    let transferred = 0;
    progress?.onStep?.(0);
    while (transferred < size) {
      const started = Date.now();
      const data = await this._recvData(binary, codes, timeout);
      await file.writeFile(data);
      transferred += data.length;
      progress?.onStep?.(transferred);
      this._sendInteger("SUCC", data.length);
      this._maxChunkTime = Math.max(this._maxChunkTime, Date.now() - started);
    }
  }
}
