import { fg } from "../../../terminalPanelHelpers.js";
import {
  isSerialProtocol,
  isTelnetProtocol,
  requiresHostKeyVerification,
  terminalTargetLine,
} from "../../../connectionProtocols.js";

function normalizeLines(lines = []) {
  const seen = new Set();
  return lines.reduce((result, line) => {
    const text = typeof line === "string" ? line.trim() : "";
    if (!text || seen.has(text)) return result;
    seen.add(text);
    result.push(text);
    return result;
  }, []);
}

function buildBlock({ title, lines, titleColor, lineColor }) {
  return [
    `${fg(titleColor)}${title}\x1b[0m`,
    ...lines.map((line) => `${fg(lineColor)}${line}\x1b[0m`),
  ]
    .join("\r\n")
    .concat("\r\n");
}

function isAutoValue(value) {
  return (
    String(value || "")
      .trim()
      .toLowerCase() === "auto"
  );
}

const STATUS_DESCRIPTOR_KEYS = Object.freeze({
  connected: "connected",
  connecting: "connecting",
  secureSession: "secureSession",
  hostKeyConfirmation: "hostKeyConfirmation",
  failed: "failed",
  closed: "closed",
});
const CLEAR_TERMINAL = "\x1b[2J\x1b[3J\x1b[H";
const STATUS_PRESENTATION = Object.freeze({
  PROGRESS: "progress",
  LIFECYCLE: "lifecycle",
});

function erasePreviousBlock(lineCount) {
  const count = Math.max(0, Number(lineCount) || 0);
  if (!count) return "";
  return `\r${"\x1b[1A\x1b[2K".repeat(count)}`;
}

export class TerminalStatusAddon {
  constructor({
    getConnection,
    getPalette,
    getFailureLabel,
    getFailureDetail,
    getStatusDetail,
    queueWrite,
    t,
  }) {
    Object.assign(this, {
      _getConnection: getConnection,
      _getPalette: getPalette,
      _getFailureLabel: getFailureLabel,
      _getFailureDetail: getFailureDetail,
      _getStatusDetail: getStatusDetail,
      _queueWrite: queueWrite,
      _t: t,
      _terminal: null,
      _fingerprint: "",
      _overlayLineCount: 0,
      _overlayAttached: false,
      _overlayPresentation: null,
      _overlayStatus: null,
    });
  }

  activate(terminal) {
    this._terminal = terminal;
  }

  dispose() {
    this.reset();
    this._terminal = null;
  }

  clear() {
    this._fingerprint = "";
    this._overlayLineCount = 0;
    this._overlayAttached = false;
    this._overlayPresentation = null;
    this._overlayStatus = null;
  }

  reset() {
    this.clear();
  }

  release() {
    const releasedProgress = this._hasProgressBlock();
    if (releasedProgress) {
      this._queueWrite(erasePreviousBlock(this._overlayLineCount), { immediate: true });
    }
    if (releasedProgress) this._fingerprint = "";
    this._overlayLineCount = 0;
    this._overlayAttached = false;
    this._overlayPresentation = null;
    this._overlayStatus = null;
  }

  write(status, detail = "") {
    const connection = this._getConnection();
    if (!this._terminal || !connection) return;

    const descriptor = this._statusDescriptor(status, detail, connection);
    if (!descriptor) return;

    // Reactive updates can replay an unchanged state (for example after a
    // locale refresh), so compare the rendered meaning before writing to xterm.
    descriptor.lines = normalizeLines(descriptor.lines);
    const fingerprint = `${status}|${descriptor.presentation}|${descriptor.title}|${descriptor.lines.join("\n")}`;
    if (fingerprint === this._fingerprint) return;
    this._fingerprint = fingerprint;
    this._render({ ...descriptor, status });
  }

  _statusDescriptor(status, detail, connection) {
    if (status === STATUS_DESCRIPTOR_KEYS.connected) {
      const palette = this._getPalette();
      return {
        title: isSerialProtocol(connection.protocol)
          ? this._t("terminal.serialConnectionOpened")
          : this._t("terminal.connectionConnected"),
        lines: [],
        titleColor: palette.success,
        lineColor: palette.hint,
        presentation: STATUS_PRESENTATION.LIFECYCLE,
      };
    }

    if (status === STATUS_DESCRIPTOR_KEYS.connecting) {
      const palette = this._getPalette();
      return {
        title: this._connectingTitle(connection),
        lines: [terminalTargetLine(connection)],
        titleColor: palette.boot,
        lineColor: palette.hint,
        clearTerminal: true,
        presentation: STATUS_PRESENTATION.PROGRESS,
      };
    }

    if (
      status === STATUS_DESCRIPTOR_KEYS.secureSession ||
      status === STATUS_DESCRIPTOR_KEYS.hostKeyConfirmation
    ) {
      const palette = this._getPalette();
      return {
        title:
          status === STATUS_DESCRIPTOR_KEYS.secureSession
            ? this._t("terminal.connectionEstablishingSecureSession")
            : this._t("terminal.connectionWaitingForHostKeyConfirmation"),
        lines: detail ? [detail] : [],
        titleColor: palette.boot,
        lineColor: palette.hint,
        clearTerminal: true,
        presentation: STATUS_PRESENTATION.PROGRESS,
      };
    }

    if (status === STATUS_DESCRIPTOR_KEYS.failed || status === STATUS_DESCRIPTOR_KEYS.closed) {
      const palette = this._getPalette();
      return {
        title:
          status === STATUS_DESCRIPTOR_KEYS.failed
            ? this._t("terminal.connectionFailed")
            : this._t("terminal.connectionClosed"),
        lines:
          status === STATUS_DESCRIPTOR_KEYS.failed
            ? [this._getFailureLabel(), detail || this._getFailureDetail()]
            : [detail || this._getStatusDetail()],
        titleColor: palette.error,
        lineColor: palette.hint,
        leadingNewline: true,
        presentation: STATUS_PRESENTATION.LIFECYCLE,
      };
    }

    return null;
  }

  _connectingTitle(connection) {
    if (requiresHostKeyVerification(connection.protocol)) {
      return this._t("terminal.connectionConnectingSsh");
    }
    if (isTelnetProtocol(connection.protocol))
      return this._t("terminal.connectionConnectingTelnet");
    if (isSerialProtocol(connection.protocol)) {
      const port = connection.serialPort || connection.host || connection.port || connection.name;
      return isAutoValue(port) || isAutoValue(connection.baudRate)
        ? this._t("terminal.connectionDetectingSerial")
        : this._t("terminal.connectionOpeningSerial");
    }
    return this._t("terminal.connectionConnecting");
  }

  _render({
    status,
    title,
    lines = [],
    titleColor,
    lineColor,
    leadingNewline = false,
    clearTerminal = false,
    presentation = STATUS_PRESENTATION.LIFECYCLE,
  }) {
    const normalized = normalizeLines(lines);
    const lineCount = 1 + normalized.length;
    const shouldErasePrevious = this._hasProgressBlock() || this._hasAttachedStatus(status);
    const cursorX = Number(this._terminal?.buffer?.active?.cursorX);
    const shouldStartOnNewLine =
      leadingNewline && !shouldErasePrevious && (!Number.isFinite(cursorX) || cursorX > 0);
    const prefix = `${clearTerminal ? CLEAR_TERMINAL : ""}${
      shouldErasePrevious ? erasePreviousBlock(this._overlayLineCount) : ""
    }${shouldStartOnNewLine ? "\r\n" : ""}`;
    const payload = `${prefix}${buildBlock({
      title,
      lines: normalized,
      titleColor,
      lineColor,
    })}`;
    this._overlayLineCount = lineCount;
    this._overlayAttached = true;
    this._overlayPresentation = presentation;
    this._overlayStatus = status;
    this._queueWrite(payload, { immediate: true });
  }

  _hasProgressBlock() {
    return this._overlayAttached && this._overlayPresentation === STATUS_PRESENTATION.PROGRESS;
  }

  _hasAttachedStatus(status) {
    return this._overlayAttached && !!status && this._overlayStatus === status;
  }
}
