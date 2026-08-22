import { asBytes, bytesToBinaryString, stringToBytes } from "./bytes";
import { DRAG_INIT_TIMEOUT, TRZSZ_TRIGGER, TRZSZ_TRIGGER_PATTERN } from "./constants";
import { checkDuplicateNames, chooseSendFiles, openSaveFile, parseDragPaths } from "./files";
import { chooseDownloadDirectory } from "./ipc";
import { TextProgressBar } from "./progress";
import { stripServerOutput } from "./text";
import { TrzszTransfer } from "./transfer";

const TRIGGER_BYTES = stringToBytes(TRZSZ_TRIGGER);

function findMagicOffset(output) {
  if (typeof output === "string") {
    const index = output.lastIndexOf(TRZSZ_TRIGGER);
    return index < 0 ? -1 : index;
  }

  const bytes = asBytes(output);
  if (bytes.length < TRIGGER_BYTES.length) return -1;
  let found = -1;
  for (let index = bytes.indexOf(0x3a); index >= 0; index = bytes.indexOf(0x3a, index + 1)) {
    if (bytes.length - index < TRIGGER_BYTES.length) break;
    let matched = true;
    for (let offset = 0; offset < TRIGGER_BYTES.length; offset += 1) {
      if (bytes[index + offset] !== TRIGGER_BYTES[offset]) {
        matched = false;
        break;
      }
    }
    if (matched) {
      found = index;
      index += TRIGGER_BYTES.length - 1;
    }
  }
  return found;
}

function findMagicKey(output) {
  const index = findMagicOffset(output);
  if (index < 0) return null;
  return typeof output === "string"
    ? output.substring(index)
    : bytesToBinaryString(asBytes(output).subarray(index));
}

function splitAtMagicKey(output) {
  const index = findMagicOffset(output);
  if (index < 0) return null;
  if (typeof output === "string") {
    return {
      prefix: output.substring(0, index),
      protocol: output.substring(index),
    };
  }

  const bytes = asBytes(output);
  return {
    prefix: bytes.subarray(0, index),
    protocol: bytes.subarray(index),
  };
}

export class LocalTrzszFilter {
  constructor({
    sendToServer,
    writeToTerminal,
    onTransferModeChange,
    terminalColumns,
    messages,
    maxDataChunkSize,
    dragInitTimeout,
    directoryUpload,
  }) {
    this._sendToServer = sendToServer;
    this._writeToTerminal = writeToTerminal;
    this._onTransferModeChange = onTransferModeChange;
    this._terminalColumns = terminalColumns || 80;
    this._messages = messages;
    this._maxDataChunkSize = maxDataChunkSize;
    this._dragInitTimeout = dragInitTimeout || DRAG_INIT_TIMEOUT;
    this._directoryUpload = directoryUpload !== false;
    this._transfer = null;
    this._progress = null;
    this._uniqueIds = new Map();
    this._uploadFiles = null;
    this._uploadResolve = null;
    this._uploadReject = null;
    this._uploadInterrupting = false;
    this._uploadSkipCommand = false;
    this._uploadToken = 0;
    this._aborted = false;
  }

  setMessages(messages) {
    this._messages = messages;
  }

  setTerminalColumns(columns) {
    this._terminalColumns = columns || 80;
    this._progress?.setTerminalColumns?.(columns);
  }

  setOptions({ maxDataChunkSize, dragInitTimeout, directoryUpload } = {}) {
    this._maxDataChunkSize = maxDataChunkSize;
    this._dragInitTimeout = dragInitTimeout || DRAG_INIT_TIMEOUT;
    this._directoryUpload = directoryUpload !== false;
  }

  isTransferringFiles() {
    return !!this._transfer;
  }

  isWaitingForUploadStart() {
    return !!this._uploadFiles;
  }

  stopTransferringFiles() {
    this._transfer?.stopTransferring?.();
  }

  abortTransfer() {
    this._aborted = true;
    this._transfer?.stopTransferring?.();
    this._uploadToken += 1;
    this._finishWaitingUpload(new Error("Transfer aborted: view deactivated"));
  }

  processServerOutput(output) {
    if (this.isTransferringFiles()) {
      this._transfer.addReceivedData(output);
      return;
    }
    if (this._uploadInterrupting) return;
    if (this._uploadSkipCommand) {
      this._uploadSkipCommand = false;
      const stripped = stripServerOutput(output);
      if (stripped === "trz" || stripped === "trz -d") {
        this._writeToTerminal("\r\n");
        return;
      }
    }

    const split = splitAtMagicKey(output);
    if (split) {
      this._detectAndHandle(split.protocol);
      const strippedPrefix = stripServerOutput(split.prefix);
      if (strippedPrefix === "trz" || strippedPrefix === "trz -d") {
        this._writeToTerminal("\r\n");
        return;
      }
      if (split.prefix.length) this._writeToTerminal(split.prefix);
      return;
    }

    this._writeToTerminal(output);
  }

  processTerminalInput(input) {
    if (this.isTransferringFiles()) {
      if (input === "\x03") this.stopTransferringFiles();
      return;
    }
    this._sendToServer(input);
  }

  processBinaryInput(input) {
    if (!this.isTransferringFiles()) this._sendToServer(stringToBytes(input));
  }

  async uploadPaths(paths) {
    return this._beginDragUpload(() => parseDragPaths(paths));
  }

  async _beginDragUpload(parseFiles) {
    if (this._uploadFiles || this.isTransferringFiles()) {
      throw new Error("The previous upload has not been completed yet");
    }
    this._uploadFiles = await parseFiles();
    if (!this._uploadFiles?.length) {
      this._uploadFiles = null;
      throw new Error("No files to upload");
    }
    const hasDirectory = this._uploadFiles.some(
      (file) => file.isDir() || file.getRelPath().length > 1,
    );
    if (hasDirectory && !this._directoryUpload) {
      this._uploadFiles = null;
      throw new Error("Directory upload is disabled");
    }
    const uploadToken = ++this._uploadToken;
    try {
      await this._onTransferModeChange?.(true);
    } catch (error) {
      this._uploadToken += 1;
      this._finishWaitingUpload();
      throw error;
    }
    this._uploadInterrupting = true;
    this._sendToServer("\x03");
    await new Promise((resolve) => setTimeout(resolve, 200));
    if (uploadToken !== this._uploadToken || !this._uploadFiles) {
      this._uploadInterrupting = false;
      await this._onTransferModeChange?.(false);
      return false;
    }
    this._uploadInterrupting = false;
    this._uploadSkipCommand = true;
    this._sendToServer(hasDirectory ? "trz -d\r" : "trz\r");
    setTimeout(() => {
      if (uploadToken !== this._uploadToken || !this._uploadFiles) return;
      this._finishWaitingUpload(new Error("Upload does not start"));
      this._onTransferModeChange?.(false);
    }, this._dragInitTimeout);
    return new Promise((resolve, reject) => {
      this._uploadResolve = resolve;
      this._uploadReject = reject;
    });
  }

  _finishWaitingUpload(error = null) {
    const reject = this._uploadReject;
    this._uploadFiles = null;
    this._uploadResolve = null;
    this._uploadReject = null;
    this._uploadInterrupting = false;
    this._uploadSkipCommand = false;
    if (error) reject?.(error);
  }

  async _detectAndHandle(output) {
    const magic = findMagicKey(output);
    if (!magic) return;
    const match = magic.match(TRZSZ_TRIGGER_PATTERN);
    if (!match) return;
    const uniqueId = match[3] || "";
    if (this._uniqueIdExists(uniqueId)) return;
    const mode = match[1];
    const remoteIsWindows =
      uniqueId === ":1" || (uniqueId.length === 14 && uniqueId.endsWith("10"));
    this._uploadToken += 1;
    this._aborted = false;
    this._transfer = new TrzszTransfer({
      sendToServer: this._sendToServer,
      maxDataChunkSize: this._maxDataChunkSize,
    });
    this._onTransferModeChange?.(true);
    try {
      if (this._aborted) return;
      if (mode === "S") await this._download(remoteIsWindows);
      if (mode === "R") await this._upload(false, remoteIsWindows);
      if (mode === "D") await this._upload(true, remoteIsWindows);
      if (!this._aborted) this._uploadResolve?.();
    } catch (error) {
      if (!this._aborted) await this._transfer.clientError(error);
      this._uploadReject?.(error);
    } finally {
      this._uploadResolve = null;
      this._uploadReject = null;
      await this._transfer.cleanup();
      this._progress?.showCursor?.();
      this._progress = null;
      this._transfer = null;
      this._onTransferModeChange?.(false);
    }
  }

  _uniqueIdExists(uniqueId) {
    if (uniqueId.length < 8) return false;
    if (uniqueId.length === 14 && uniqueId.endsWith("00")) return false;
    if (this._uniqueIds.has(uniqueId)) return true;
    if (this._uniqueIds.size >= 100) {
      const kept = new Map();
      for (const [key, value] of this._uniqueIds) {
        if (value >= 50) kept.set(key, value - 50);
      }
      this._uniqueIds = kept;
    }
    this._uniqueIds.set(uniqueId, this._uniqueIds.size);
    return false;
  }

  _createProgress(config) {
    if (config.quiet === true) {
      this._progress = null;
      return;
    }
    this._progress = new TextProgressBar(
      this._writeToTerminal,
      this._terminalColumns,
      config.tmux_pane_width,
    );
    this._progress.hideCursor();
  }

  async _download(remoteIsWindows) {
    const directory = await chooseDownloadDirectory({
      title: this._messages.chooseDownloadDirectoryTitle,
    });
    if (!directory) {
      this._transfer.sendAction(false, remoteIsWindows);
      return;
    }
    this._transfer.sendAction(true, remoteIsWindows);
    const config = await this._transfer.recvConfig();
    this._createProgress(config);
    const localNames = await this._transfer.recvFiles(
      { root: directory, maps: new Map(), openSaveFile },
      this._progress,
    );
    await this._transfer.clientExit(this._messages.formatSavedFiles(localNames, directory.name));
  }

  async _upload(directory, remoteIsWindows) {
    if (directory && !this._directoryUpload) {
      this._transfer.sendAction(false, remoteIsWindows);
      return;
    }
    const files =
      this._uploadFiles || (await chooseSendFiles({ directory, messages: this._messages }));
    this._uploadFiles = null;
    if (!files?.length) {
      this._transfer.sendAction(false, remoteIsWindows);
      return;
    }
    this._transfer.sendAction(true, remoteIsWindows);
    const config = await this._transfer.recvConfig();
    if (config.overwrite === true) checkDuplicateNames(files);
    this._createProgress(config);
    const remoteNames = await this._transfer.sendFiles(files, this._progress);
    await this._transfer.clientExit(this._messages.formatSavedFiles(remoteNames, ""));
  }
}
