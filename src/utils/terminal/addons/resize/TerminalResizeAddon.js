const FIT_SETTLE_MS = 48;
const FONT_METRICS_SETTLE_MS = 80;
const BACKEND_RESIZE_DEBOUNCE_MS = 96;

function parseSize(style, propertyName) {
  return Number.parseInt(style.getPropertyValue(propertyName), 10) || 0;
}

function readMountContentSize(mount) {
  const style = window.getComputedStyle(mount);
  const paddingHorizontal = parseSize(style, "padding-left") + parseSize(style, "padding-right");
  const paddingVertical = parseSize(style, "padding-top") + parseSize(style, "padding-bottom");

  return {
    width: Math.max(0, mount.clientWidth - paddingHorizontal),
    height: Math.max(0, mount.clientHeight - paddingVertical),
  };
}

function proposeGeometry(terminal, mount) {
  if (!terminal?.element) return undefined;
  const renderDimensions = terminal._core?._renderService?.dimensions;
  const cellWidth = renderDimensions?.css?.cell?.width ?? 0;
  const cellHeight = renderDimensions?.css?.cell?.height ?? 0;
  if (cellWidth === 0 || cellHeight === 0) return undefined;

  const mountSize = readMountContentSize(mount);
  if (mountSize.width === 0 || mountSize.height === 0) return undefined;

  return {
    cols: Math.max(2, Math.floor(mountSize.width / cellWidth)),
    rows: Math.max(1, Math.floor(mountSize.height / cellHeight)),
  };
}

function readResizeObserverEntrySize(entry) {
  const box = Array.isArray(entry.borderBoxSize) ? entry.borderBoxSize[0] : entry.borderBoxSize;
  if (box) {
    return {
      width: Math.round(box.inlineSize),
      height: Math.round(box.blockSize),
    };
  }

  return {
    width: Math.round(entry.contentRect.width),
    height: Math.round(entry.contentRect.height),
  };
}

export class TerminalResizeAddon {
  constructor({
    getMount,
    getSessionId,
    onFrontendResize,
    onBackendResize,
    canSyncBackend,
    isEnabled,
    isDisposed,
  }) {
    this._getMount = getMount;
    this._getSessionId = getSessionId;
    this._onFrontendResize = onFrontendResize;
    this._onBackendResize = onBackendResize;
    this._canSyncBackend = canSyncBackend || (() => true);
    this._isEnabled = isEnabled;
    this._isDisposed = isDisposed;

    this._terminal = null;
    this._resizeDisposable = null;
    this._resizeObserver = null;
    this._observedMount = null;
    this._fitTimer = undefined;
    this._fitFrame = undefined;
    this._pendingFitForce = false;
    this._pendingPixelBackendSync = false;
    this._pendingFontMetricsTimer = undefined;
    this._backendResizeTimer = undefined;
    this._pendingBackendSize = null;
    this._lastObservedSize = { width: 0, height: 0 };
    this._lastProposedGeometry = { cols: 0, rows: 0 };
    this._lastSyncedBackendSize = { sessionId: "", cols: 0, rows: 0, widthPx: 0, heightPx: 0 };
  }

  activate(terminal) {
    this._terminal = terminal;
    this._resizeDisposable = terminal.onResize((size) => {
      this._handleTerminalResize(size);
    });
    this._resizeObserver = new ResizeObserver((entries) => {
      const nextSize = readResizeObserverEntrySize(entries[0]);
      this.handleObservedResize(nextSize);
    });
  }

  dispose() {
    this.reset();
    this._resizeDisposable?.dispose?.();
    this._resizeDisposable = null;
    this._resizeObserver?.disconnect?.();
    this._resizeObserver = null;
    this._observedMount = null;
    this._terminal = null;
  }

  observe() {
    const mount = this._getMount();
    if (!this._resizeObserver) return;
    if (!mount || !this._isEnabled()) {
      if (this._observedMount) this._resizeObserver.disconnect();
      this._observedMount = null;
      return;
    }
    if (this._observedMount === mount) return;
    this._resizeObserver.disconnect();
    this._resizeObserver.observe(mount);
    this._observedMount = mount;
  }

  fitIfNeeded({ force = false } = {}) {
    const terminal = this._terminal;
    const mount = this._getMount();
    if (!terminal || !mount) {
      return false;
    }

    const size = this._hasObservedSize()
      ? this._lastObservedSize
      : readResizeObserverEntrySize({ contentRect: mount.getBoundingClientRect() });
    if (size.width === 0 || size.height === 0) {
      return false;
    }

    const geometry = proposeGeometry(terminal, mount);
    if (!geometry) {
      return false;
    }

    const unchanged =
      geometry.cols === this._lastProposedGeometry.cols &&
      geometry.rows === this._lastProposedGeometry.rows &&
      geometry.cols === terminal.cols &&
      geometry.rows === terminal.rows;

    if (!force && unchanged) {
      return false;
    }

    this._lastProposedGeometry = geometry;
    terminal.resize(geometry.cols, geometry.rows);
    return true;
  }

  scheduleFit({ immediate = false, force = false } = {}) {
    this._pendingFitForce = this._pendingFitForce || force;

    if (immediate) {
      clearTimeout(this._fitTimer);
      cancelAnimationFrame(this._fitFrame);
      this._fitFrame = requestAnimationFrame(() => this._flushFit());
      return;
    }

    if (!this._fitFrame) {
      this._fitFrame = requestAnimationFrame(() => this._flushFit());
    }

    clearTimeout(this._fitTimer);
    this._fitTimer = setTimeout(() => {
      if (this._isDisposed() || this._fitFrame) return;
      this._pendingPixelBackendSync = true;
      this._fitFrame = requestAnimationFrame(() => this._flushFit());
    }, FIT_SETTLE_MS);
  }

  scheduleFontMetricsRefit() {
    this._lastProposedGeometry = { cols: 0, rows: 0 };
    this.scheduleFit({ immediate: true, force: true });

    if (document.fonts?.ready) {
      document.fonts.ready.then(() => {
        if (!this._isDisposed()) {
          this.scheduleFit({ immediate: true, force: true });
        }
      });
    }

    clearTimeout(this._pendingFontMetricsTimer);
    this._pendingFontMetricsTimer = setTimeout(() => {
      this._pendingFontMetricsTimer = undefined;
      if (!this._isDisposed()) {
        this.scheduleFit({ immediate: true, force: true });
      }
    }, FONT_METRICS_SETTLE_MS);
  }

  handleObservedResize(size) {
    if (size.width <= 0 || size.height <= 0) {
      this._lastObservedSize = size;
      return;
    }

    if (
      size.width === this._lastObservedSize.width &&
      size.height === this._lastObservedSize.height
    ) {
      return;
    }

    this._lastObservedSize = size;
    this.scheduleFit();
  }

  queueBackendSync(size = null, { immediate = false } = {}) {
    const terminal = this._terminal;
    const sessionId = this._getSessionId();
    if (!terminal || !sessionId || !this._canSyncBackend()) return;

    const nextSize = size ?? { cols: terminal.cols, rows: terminal.rows };
    if (!nextSize?.cols || !nextSize?.rows) return;
    this._pendingBackendSize = {
      sessionId,
      cols: nextSize.cols,
      rows: nextSize.rows,
    };

    if (immediate) {
      this._flushBackendSync();
      return;
    }

    clearTimeout(this._backendResizeTimer);
    this._backendResizeTimer = setTimeout(
      () => this._flushBackendSync(),
      BACKEND_RESIZE_DEBOUNCE_MS,
    );
  }

  resetBackendSyncState() {
    clearTimeout(this._backendResizeTimer);
    this._backendResizeTimer = undefined;
    this._pendingBackendSize = null;
    this._lastSyncedBackendSize = { sessionId: "", cols: 0, rows: 0, widthPx: 0, heightPx: 0 };
  }

  reset() {
    clearTimeout(this._fitTimer);
    this._fitTimer = undefined;
    cancelAnimationFrame(this._fitFrame);
    this._fitFrame = undefined;
    clearTimeout(this._pendingFontMetricsTimer);
    this._pendingFontMetricsTimer = undefined;
    this._pendingFitForce = false;
    this._pendingPixelBackendSync = false;
    this._lastObservedSize = { width: 0, height: 0 };
    this._lastProposedGeometry = { cols: 0, rows: 0 };
    this.resetBackendSyncState();
  }

  _hasObservedSize() {
    return this._lastObservedSize.width > 0 && this._lastObservedSize.height > 0;
  }

  _readPixelSize() {
    if (this._hasObservedSize()) {
      return {
        widthPx: this._lastObservedSize.width,
        heightPx: this._lastObservedSize.height,
      };
    }

    const mount = this._getMount();
    if (!mount) {
      return { widthPx: 0, heightPx: 0 };
    }

    const rect = mount.getBoundingClientRect();
    return {
      widthPx: Math.max(0, Math.round(rect.width)),
      heightPx: Math.max(0, Math.round(rect.height)),
    };
  }

  _flushFit() {
    this._fitFrame = undefined;
    const force = this._pendingFitForce;
    const syncPixels = this._pendingPixelBackendSync;
    this._pendingFitForce = false;
    this._pendingPixelBackendSync = false;
    const didResize = this.fitIfNeeded({ force });
    if (!didResize && syncPixels) {
      this.queueBackendSync(null);
    }
  }

  _syncBackendSize(size = null) {
    const terminal = this._terminal;
    const sessionId = size?.sessionId || this._getSessionId();
    if (!terminal || !sessionId || !this._canSyncBackend()) return;

    const nextSize = size ?? { cols: terminal.cols, rows: terminal.rows };
    if (!nextSize?.cols || !nextSize?.rows) return;

    const snapshot = {
      sessionId,
      cols: nextSize.cols,
      rows: nextSize.rows,
      ...this._readPixelSize(),
    };

    const unchanged =
      snapshot.sessionId === this._lastSyncedBackendSize.sessionId &&
      snapshot.cols === this._lastSyncedBackendSize.cols &&
      snapshot.rows === this._lastSyncedBackendSize.rows &&
      snapshot.widthPx === this._lastSyncedBackendSize.widthPx &&
      snapshot.heightPx === this._lastSyncedBackendSize.heightPx;

    if (unchanged) {
      return;
    }

    if (this._onBackendResize(snapshot) === false) return;
    this._lastSyncedBackendSize = snapshot;
  }

  _flushBackendSync() {
    clearTimeout(this._backendResizeTimer);
    this._backendResizeTimer = undefined;
    if (!this._pendingBackendSize) return;
    const nextSize = this._pendingBackendSize;
    this._pendingBackendSize = null;
    this._syncBackendSize(nextSize);
  }

  _handleTerminalResize(size) {
    const payload = {
      ...size,
      ...this._readPixelSize(),
    };
    this._onFrontendResize(payload);
    this.queueBackendSync(size);
  }
}
