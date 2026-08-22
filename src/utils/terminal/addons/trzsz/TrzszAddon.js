import {
  asBytes,
  bytesIncludeAscii,
  bytesToBase64,
  bytesToBinaryString,
  payloadBytes,
} from "./bytes";
import { TRZSZ_TRIGGER } from "./constants";
import { LocalTrzszFilter } from "./filter";
import { formatSavedFiles } from "./text";
import { connectionCan } from "../../../connectionCapabilities";

const DEFAULT_TRZSZ_MESSAGES = Object.freeze({
  chooseUploadTitle: "Choose files to transfer",
  chooseUploadDirectoryTitle: "Choose folder to transfer",
  chooseDownloadDirectoryTitle: "Choose transfer download folder",
  allFilesLabel: "All files",
  formatSavedFiles,
});

export class TrzszAddon {
  constructor({
    getSessionContext,
    sendText,
    sendBytes,
    setRawOutput,
    writeTerminal,
    enabled = true,
    messages = {},
    maxDataChunkSize,
    dragInitTimeout,
    directoryUpload,
  }) {
    this._terminal = null;
    this._enabled = enabled !== false;
    this._getSessionContext = getSessionContext;
    this._sendText = sendText;
    this._sendBytes = sendBytes;
    this._setRawOutput = setRawOutput;
    this._writeTerminal = writeTerminal;
    this._options = { maxDataChunkSize, dragInitTimeout, directoryUpload };
    this._resizeDisposable = null;
    this._filter = null;
    this._rawOutputEnabled = false;
    this._messageOverrides = null;
    this.setMessages(messages);
  }

  activate(terminal) {
    this._terminal = terminal;
    this._filter = new LocalTrzszFilter({
      terminalColumns: terminal?.cols || 80,
      messages: this._messages,
      ...this._options,
      writeToTerminal: (output) =>
        this._writeTerminal(
          typeof output === "string" ? output : bytesToBinaryString(asBytes(output)),
          { recordable: true },
        ),
      sendToServer: (input) =>
        typeof input === "string"
          ? this._sendText(input)
          : this._sendBytes(bytesToBase64(asBytes(input))),
      onTransferModeChange: (enabled) => {
        return this._setRawOutputMode(enabled);
      },
    });
    this._resizeDisposable = terminal.onResize(({ cols }) => {
      this._filter?.setTerminalColumns(cols);
    });
  }

  dispose() {
    this._resizeDisposable?.dispose?.();
    this._resizeDisposable = null;
    this._filter?.abortTransfer?.();
    this._filter = null;
    this._setRawOutputMode(false);
    this._terminal = null;
  }

  stopTransfer() {
    this._filter?.abortTransfer?.();
    this._setRawOutputMode(false);
  }

  isEnabled() {
    return this._enabled !== false;
  }

  setEnabled(enabled) {
    this._enabled = enabled !== false;
  }

  setMessages(messages = {}) {
    if (messages === this._messageOverrides) return;
    this._messageOverrides = messages;
    this._messages = {
      ...DEFAULT_TRZSZ_MESSAGES,
      ...messages,
    };
    this._filter?.setMessages?.(this._messages);
  }

  setOptions(options = {}) {
    this._options = { ...this._options, ...options };
    this._filter?.setOptions?.(this._options);
  }

  processServerOutput(payload) {
    if (!this._canProcess()) return false;
    if (this._filter.isTransferringFiles() || this._filter.isWaitingForUploadStart()) {
      const bytes = payloadBytes(payload);
      this._filter.processServerOutput(bytes);
      return true;
    }

    const text = String(payload?.data || "");
    if (text.includes(TRZSZ_TRIGGER)) {
      this._setRawOutputMode(true);
      this._filter.processServerOutput(text);
      return true;
    }

    if (payload?.dataBase64) {
      const bytes = payloadBytes(payload);
      if (bytesIncludeAscii(bytes, TRZSZ_TRIGGER)) {
        this._setRawOutputMode(true);
        this._filter.processServerOutput(bytes);
        return true;
      }
    }

    return false;
  }

  processTerminalInput(data) {
    return this._processWhileTransferring("processTerminalInput", data);
  }

  processBinaryInput(data) {
    return this._processWhileTransferring("processBinaryInput", data);
  }

  uploadPaths(paths) {
    if (!this._canProcess() || !paths?.length) return Promise.resolve(false);
    return this._filter.uploadPaths(paths).then(() => true);
  }

  _canProcess() {
    const context = this._getSessionContext?.();
    return !!this._filter && context?.active && connectionCan(context, "sftp") && this.isEnabled();
  }

  _setRawOutputMode(enabled) {
    if (!connectionCan(this._getSessionContext?.(), "rawOutput")) {
      this._rawOutputEnabled = false;
      return;
    }
    const nextEnabled = enabled === true;
    if (this._rawOutputEnabled === nextEnabled) return;
    this._rawOutputEnabled = nextEnabled;
    return this._setRawOutput?.({ enabled: nextEnabled })?.catch?.(() => {
      this._rawOutputEnabled = !nextEnabled;
      if (nextEnabled) throw new Error("Failed to enable raw terminal output");
    });
  }

  _processWhileTransferring(method, data) {
    if (!this._canProcess() || !this._filter.isTransferringFiles()) return false;
    this._filter[method](data);
    return true;
  }
}
