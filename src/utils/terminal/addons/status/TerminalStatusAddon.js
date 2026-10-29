import { fg } from "../../../terminalPanelHelpers.js";
import { getTerminalTextWidth } from "../../unicodeWidth.js";
import {
  isSerialProtocol,
  isTelnetProtocol,
  requiresHostKeyVerification,
  terminalTargetLine,
} from "../../../connectionProtocols.js";

function normalizeLines(lines = []) {
  const seen = new Set();
  const result = [];
  for (const line of lines) {
    if (typeof line !== "string") continue;
    for (const part of line.split(/\r\n|\r|\n/)) {
      const text = part.trim();
      if (!text || seen.has(text)) continue;
      seen.add(text);
      result.push(text);
    }
  }
  return result;
}

function countWrappedRows(value, columns, startColumn = 0) {
  return Math.max(1, Math.ceil((startColumn + getTerminalTextWidth(value)) / columns));
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
    getDetectedBaudRate,
    queueWrite,
    t,
  }) {
    Object.assign(this, {
      _getConnection: getConnection,
      _getPalette: getPalette,
      _getFailureLabel: getFailureLabel,
      _getFailureDetail: getFailureDetail,
      _getStatusDetail: getStatusDetail,
      _getDetectedBaudRate: getDetectedBaudRate,
      _queueWrite: queueWrite,
      _t: t,
      _terminal: null,
      _resizeDisposable: null,
      _fingerprint: "",
      _releasedLifecycleStatus: null,
      _overlayLineCount: 0,
      _overlayAttached: false,
      _overlayPresentation: null,
      _overlayStatus: null,
      _overlayTitle: "",
      _overlayLines: [],
      _overlayFirstColumn: 0,
    });
  }

  activate(terminal) {
    this._resizeDisposable?.dispose?.();
    this._terminal = terminal;
    const resizeSubscription = terminal?.onResize?.(() => this._refreshOverlayLineCount());
    this._resizeDisposable = resizeSubscription?.dispose ? resizeSubscription : null;
  }

  dispose() {
    this._resizeDisposable?.dispose?.();
    this._resizeDisposable = null;
    this.reset();
    this._terminal = null;
  }

  clear() {
    this._fingerprint = "";
    this._releasedLifecycleStatus = null;
    this._overlayLineCount = 0;
    this._overlayAttached = false;
    this._overlayPresentation = null;
    this._overlayStatus = null;
    this._overlayTitle = "";
    this._overlayLines = [];
    this._overlayFirstColumn = 0;
  }

  reset() {
    this.clear();
  }

  release() {
    const releasedProgress = this._hasProgressBlock();
    if (releasedProgress) {
      this._queueWrite(erasePreviousBlock(this._overlayLineCount));
    }
    if (releasedProgress) this._fingerprint = "";
    // lifecycle 横幅被终端输出"消费"后记住它的状态：同状态的本地化重放
    // （locale 切换）不再写回光标处，但后续不同状态（如 closed）仍要呈现。
    // 没有附着横幅的重复 release 不改写这个标记。
    if (this._overlayAttached) {
      this._releasedLifecycleStatus = releasedProgress ? null : this._overlayStatus;
    }
    this._overlayLineCount = 0;
    this._overlayAttached = false;
    this._overlayPresentation = null;
    this._overlayStatus = null;
    this._overlayTitle = "";
    this._overlayLines = [];
    this._overlayFirstColumn = 0;
  }

  _refreshOverlayLineCount() {
    if (!this._overlayAttached) return;
    this._overlayLineCount = this._countOverlayLines();
  }

  _countOverlayLines() {
    const columns = Math.max(1, Number(this._terminal?.cols) || 80);
    let lineCount = countWrappedRows(this._overlayTitle, columns, this._overlayFirstColumn);
    for (const line of this._overlayLines) {
      lineCount += countWrappedRows(line, columns);
    }
    return lineCount;
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
    // lifecycle 横幅是事件而非持续状态：同一状态被终端输出 release 后，
    // 本地化重放（locale 切换等）不应再把它写进当前光标处，否则会污染
    // 提示行与 scrollback。首写与仍附着时的原地替换不受影响；不同状态
    // （如 connected 之后的 closed）仍正常呈现。
    if (
      descriptor.presentation === STATUS_PRESENTATION.LIFECYCLE &&
      !this._overlayAttached &&
      this._releasedLifecycleStatus === status
    ) {
      return;
    }
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
        lines:
          isSerialProtocol(connection.protocol) &&
          isAutoValue(connection.baudRate) &&
          Number(this._getDetectedBaudRate()) > 0
            ? [
                this._t("terminal.serialAutoBaudDetected", {
                  baudRate: this._getDetectedBaudRate() || "",
                }),
              ]
            : [],
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
    const shouldErasePrevious = this._hasProgressBlock() || this._hasAttachedStatus(status);
    // 断开/失败提示前始终补一个换行：即使光标已在行首，也与上方会话输出
    // 之间留出空行分隔；仅在覆盖前一块状态（重连进度等）时不追加。
    const shouldStartOnNewLine = leadingNewline && !shouldErasePrevious;
    const startsAtBlockOrigin = clearTerminal || shouldErasePrevious || shouldStartOnNewLine;
    const firstColumn = startsAtBlockOrigin
      ? 0
      : Number(this._terminal?.buffer?.active?.cursorX) || 0;
    const prefix = `${clearTerminal ? CLEAR_TERMINAL : ""}${
      shouldErasePrevious ? erasePreviousBlock(this._overlayLineCount) : ""
    }${shouldStartOnNewLine ? "\r\n" : ""}`;
    const payload = `${prefix}${buildBlock({
      title,
      lines: normalized,
      titleColor,
      lineColor,
    })}`;
    this._overlayTitle = title;
    this._overlayLines = normalized;
    this._overlayFirstColumn = firstColumn;
    this._overlayLineCount = this._countOverlayLines();
    this._overlayAttached = true;
    this._releasedLifecycleStatus = null;
    this._overlayPresentation = presentation;
    this._overlayStatus = status;
    this._queueWrite(payload);
  }

  _hasProgressBlock() {
    return this._overlayAttached && this._overlayPresentation === STATUS_PRESENTATION.PROGRESS;
  }

  _hasAttachedStatus(status) {
    return this._overlayAttached && !!status && this._overlayStatus === status;
  }
}
