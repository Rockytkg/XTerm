import { registerLineDecorations } from "./lineDecorations.js";
import { compileHighlightRuleSet } from "./ruleCompiler.js";

const OVERSCAN_ROWS = 4;
const MAX_MATCHES_PER_LINE = 32;
const ROWS_PER_FRAME = 16;

function disposeAll(disposables) {
  for (const disposable of disposables) disposable?.dispose?.();
}

export class TerminalHighlightAddon {
  constructor({ getEnabled }) {
    this._getEnabled = getEnabled;
    this._terminal = null;
    this._subscriptions = [];
    this._frameId = undefined;
    this._pendingRows = new Set();
    this._decoratedRows = new Map();
    this._analysisCache = new Map();
    this._ruleSet = compileHighlightRuleSet([], "");
    this._rulesChangedWhilePaused = false;
  }

  activate(terminal) {
    this._terminal = terminal;
    this._syncScanning();
  }

  dispose() {
    this._stopScanning();
    this._clearRows();
    this._terminal = null;
  }

  setRules({ rules = [], hash = "" } = {}) {
    const nextHash = String(hash || "");
    const unchanged = nextHash !== "" && nextHash === this._ruleSet.hash;
    if (!unchanged) {
      this._ruleSet = compileHighlightRuleSet(rules, nextHash);
      this._rulesChangedWhilePaused = true;
    }

    this._syncScanning();
    if (this._ruleSet.ruleCount === 0) {
      this._rulesChangedWhilePaused = false;
      this._clearRows();
      return;
    }
    if (!this._canScan()) return;

    if (this._rulesChangedWhilePaused) {
      this._rulesChangedWhilePaused = false;
      this.invalidate();
    } else {
      this._replacePendingWithViewport();
    }
  }

  invalidate() {
    this._clearRows();
    this._replacePendingWithViewport();
  }

  _canScan() {
    return !!this._terminal && this._ruleSet.ruleCount > 0 && this._getEnabled();
  }

  _syncScanning() {
    if (!this._canScan()) {
      this._stopScanning();
      return;
    }
    if (this._subscriptions.length > 0) return;

    const terminal = this._terminal;
    this._subscriptions = [
      terminal.onWriteParsed(() => this._handleWrite()),
      terminal.onRender((event) => this._enqueueRenderedRows(event)),
      terminal.onResize(() => this.invalidate()),
      terminal.onScroll(() => this._replacePendingWithViewport()),
    ];
    this._replacePendingWithViewport();
  }

  _stopScanning() {
    disposeAll(this._subscriptions);
    this._subscriptions = [];
    this._pendingRows.clear();
    if (this._frameId === undefined) return;
    cancelAnimationFrame(this._frameId);
    this._frameId = undefined;
  }

  _handleWrite() {
    // Row indexes in a negative cache cannot follow scrollback trimming.
    // Positive highlights use markers and are synchronized before scanning.
    this._analysisCache.clear();
    this._synchronizeDecoratedRows();
    this._enqueueViewport();
  }

  _replacePendingWithViewport() {
    if (!this._canScan()) return;
    this._pendingRows.clear();
    this._synchronizeDecoratedRows();
    this._enqueueViewport();
  }

  _enqueueViewport() {
    const terminal = this._terminal;
    if (!terminal) return;
    const { active: buffer } = terminal.buffer;
    this._enqueueRange(
      buffer.viewportY - OVERSCAN_ROWS,
      buffer.viewportY + terminal.rows + OVERSCAN_ROWS,
    );
  }

  _enqueueRenderedRows({ start = 0, end = start } = {}) {
    const terminal = this._terminal;
    if (!terminal) return;
    const viewportY = terminal.buffer.active.viewportY;
    this._enqueueRange(viewportY + start, viewportY + end);
  }

  _enqueueRange(firstRow, lastRow) {
    if (!this._canScan()) return;
    const lastBufferRow = this._terminal.buffer.active.length - 1;
    const start = Math.max(0, Number.isFinite(firstRow) ? firstRow : 0);
    const end = Math.min(lastBufferRow, Number.isFinite(lastRow) ? lastRow : start);
    if (end < start) return;
    for (let row = start; row <= end; row += 1) this._pendingRows.add(row);
    this._scheduleFrame();
  }

  _scheduleFrame() {
    if (this._frameId !== undefined || this._pendingRows.size === 0) return;
    this._frameId = requestAnimationFrame(() => {
      this._frameId = undefined;
      this._processFrame();
    });
  }

  _processFrame() {
    if (!this._canScan()) return;
    const buffer = this._terminal.buffer.active;
    this._synchronizeDecoratedRows();

    let processed = 0;
    for (const row of this._pendingRows) {
      this._pendingRows.delete(row);
      this._refreshRow(row, buffer);
      if (++processed >= ROWS_PER_FRAME) break;
    }

    this._pruneAnalysisCache();
    this._scheduleFrame();
  }

  _refreshRow(row, buffer) {
    const terminal = this._terminal;
    const line = buffer.getLine(row);
    if (!line) {
      this._removeRow(row);
      return;
    }

    const text = line.translateToString(false, 0, terminal.cols);
    if (this._isCurrent(this._decoratedRows.get(row), text)) return;
    if (this._isCurrent(this._analysisCache.get(row), text)) return;
    this._removeRow(row);

    if (!text.trim()) {
      this._cacheAnalysis(row, text);
      return;
    }

    const matches = this._ruleSet.collectMatches(text, MAX_MATCHES_PER_LINE);
    if (matches.length === 0) {
      this._cacheAnalysis(row, text);
      return;
    }

    const marker = terminal.registerMarker(row - (buffer.baseY + buffer.cursorY));
    if (!marker) return;
    const decorations = registerLineDecorations({
      terminal,
      marker,
      line,
      matches,
      limit: MAX_MATCHES_PER_LINE,
    });
    if (decorations.length === 0) {
      marker.dispose();
      this._cacheAnalysis(row, text);
      return;
    }

    this._storeDecoratedRow({ row, text, marker, decorations });
  }

  _isCurrent(entry, text) {
    return entry?.hash === this._ruleSet.hash && entry.text === text;
  }

  _cacheAnalysis(row, text) {
    this._analysisCache.set(row, { hash: this._ruleSet.hash, text });
  }

  _storeDecoratedRow({ row, text, marker, decorations }) {
    const entry = {
      decorations,
      disposed: false,
      hash: this._ruleSet.hash,
      marker,
      markerSubscription: null,
      row,
      text,
    };
    entry.markerSubscription = marker.onDispose(() => this._disposeDecoratedRow(entry, false));
    this._decoratedRows.set(row, entry);
  }

  _removeRow(row) {
    this._analysisCache.delete(row);
    const entry = this._decoratedRows.get(row);
    if (entry) this._disposeDecoratedRow(entry);
  }

  _disposeDecoratedRow(entry, disposeMarker = true) {
    if (!entry || entry.disposed) return;
    entry.disposed = true;
    if (this._decoratedRows.get(entry.row) === entry) this._decoratedRows.delete(entry.row);
    entry.markerSubscription?.dispose();
    disposeAll(entry.decorations);
    if (disposeMarker && !entry.marker.isDisposed) entry.marker.dispose();
  }

  _synchronizeDecoratedRows() {
    const bufferLength = this._terminal?.buffer.active.length || 0;
    if (this._decoratedRows.size === 0) return;

    const synchronized = new Map();
    for (const entry of this._decoratedRows.values()) {
      const row = entry.marker.line;
      if (entry.marker.isDisposed || !Number.isInteger(row) || row < 0 || row >= bufferLength) {
        this._disposeDecoratedRow(entry);
        continue;
      }
      entry.row = row;
      const replaced = synchronized.get(row);
      if (replaced && replaced !== entry) this._disposeDecoratedRow(replaced);
      synchronized.set(row, entry);
    }
    this._decoratedRows = synchronized;
  }

  _pruneAnalysisCache() {
    const terminal = this._terminal;
    if (!terminal || this._analysisCache.size === 0) return;
    const viewportY = terminal.buffer.active.viewportY;
    const firstRow = Math.max(0, viewportY - OVERSCAN_ROWS);
    const lastRow = viewportY + terminal.rows + OVERSCAN_ROWS;
    for (const row of this._analysisCache.keys()) {
      if (row < firstRow || row > lastRow) this._analysisCache.delete(row);
    }
  }

  _clearRows() {
    this._analysisCache.clear();
    for (const entry of [...this._decoratedRows.values()]) this._disposeDecoratedRow(entry);
    this._decoratedRows.clear();
  }
}
