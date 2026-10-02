import assert from "node:assert/strict";
import test, { mock } from "node:test";

import { TerminalResizeAddon } from "../src/utils/terminal/addons/resize/TerminalResizeAddon.js";

function createAddon({ enabled = true, onBackendResize = () => true } = {}) {
  const terminal = {
    cols: 80,
    rows: 24,
    dimensions: { css: { cell: { width: 10, height: 5 } } },
    element: {},
    resizeCalls: [],
    resize(cols, rows) {
      this.cols = cols;
      this.rows = rows;
      this.resizeCalls.push({ cols, rows });
    },
  };
  const mount = {
    clientWidth: 100,
    clientHeight: 50,
    getBoundingClientRect: () => ({ width: 100, height: 50 }),
  };
  const addon = new TerminalResizeAddon({
    getMount: () => mount,
    getSessionId: () => "session-1",
    onFrontendResize: () => {},
    onBackendResize,
    canSyncBackend: () => true,
    isEnabled: () => enabled,
    isDisposed: () => false,
  });
  addon._terminal = terminal;
  return { addon, mount, terminal };
}

function withWindow(callback) {
  const previousWindow = globalThis.window;
  globalThis.window = {
    getComputedStyle: () => ({
      getPropertyValue: (name) =>
        ({
          "padding-left": "0px",
          "padding-right": "0px",
          "padding-top": "0px",
          "padding-bottom": "0px",
          "--terminal-mount-inset-inline": "4px",
          "--terminal-mount-inset-block": "4px",
        })[name] || "",
    }),
  };
  try {
    return callback();
  } finally {
    if (previousWindow === undefined) delete globalThis.window;
    else globalThis.window = previousWindow;
  }
}

test("fit uses public xterm dimensions and subtracts the terminal content inset", () => {
  withWindow(() => {
    const { addon, terminal } = createAddon();

    assert.equal(addon.fitIfNeeded(), true);
    assert.deepEqual(terminal.resizeCalls, [{ cols: 9, rows: 8 }]);
  });
});

test("a background terminal never applies a queued fit", () => {
  withWindow(() => {
    const { addon, terminal } = createAddon({ enabled: false });

    assert.equal(addon.fitIfNeeded(), false);
    assert.deepEqual(terminal.resizeCalls, []);
  });
});

test("backend synchronization includes pixel-only changes", () => {
  const snapshots = [];
  const { addon } = createAddon({
    onBackendResize: (snapshot) => {
      snapshots.push(snapshot);
      return true;
    },
  });

  addon._lastObservedSize = { width: 100, height: 50 };
  addon.queueBackendSync({ cols: 80, rows: 24 }, { immediate: true });
  addon._lastObservedSize = { width: 120, height: 50 };
  addon.queueBackendSync({ cols: 80, rows: 24 }, { immediate: true });

  assert.deepEqual(
    snapshots.map(({ cols, rows, widthPx, heightPx }) => ({ cols, rows, widthPx, heightPx })),
    [
      { cols: 80, rows: 24, widthPx: 100, heightPx: 50 },
      { cols: 80, rows: 24, widthPx: 120, heightPx: 50 },
    ],
  );
});

test("scheduled refits do not sync pixels without an observed mount resize", () => {
  withWindow(() => {
    const snapshots = [];
    const { addon } = createAddon({
      onBackendResize: (snapshot) => {
        snapshots.push(snapshot);
        return true;
      },
    });

    addon._lastObservedSize = { width: 100, height: 50 };
    addon._pendingPixelBackendSync = false;
    addon._flushFit();

    assert.deepEqual(snapshots, []);
  });
});

test("delayed refit settling does not enable pixel synchronization", () => {
  withWindow(() => {
    const previousRequestAnimationFrame = globalThis.requestAnimationFrame;
    const previousCancelAnimationFrame = globalThis.cancelAnimationFrame;
    globalThis.requestAnimationFrame = () => 1;
    globalThis.cancelAnimationFrame = () => {};
    mock.timers.enable({ apis: ["setTimeout"] });
    try {
      const { addon } = createAddon();
      addon.scheduleFit();
      mock.timers.tick(48);

      assert.equal(addon._pendingPixelBackendSync, false);
      addon.reset();
    } finally {
      mock.timers.reset();
      if (previousRequestAnimationFrame === undefined) delete globalThis.requestAnimationFrame;
      else globalThis.requestAnimationFrame = previousRequestAnimationFrame;
      if (previousCancelAnimationFrame === undefined) delete globalThis.cancelAnimationFrame;
      else globalThis.cancelAnimationFrame = previousCancelAnimationFrame;
    }
  });
});

test("observed mount resize still synchronizes pixel-only changes", () => {
  withWindow(() => {
    const previousRequestAnimationFrame = globalThis.requestAnimationFrame;
    const previousCancelAnimationFrame = globalThis.cancelAnimationFrame;
    globalThis.requestAnimationFrame = () => 1;
    globalThis.cancelAnimationFrame = () => {};
    try {
      const snapshots = [];
      const { addon, mount, terminal } = createAddon({
        onBackendResize: (snapshot) => {
          snapshots.push(snapshot);
          return true;
        },
      });

      mount.clientWidth = 809;
      mount.clientHeight = 128;
      addon._lastObservedSize = { width: 808, height: 128 };
      addon._lastProposedGeometry = { cols: 80, rows: 24 };
      terminal.cols = 80;
      terminal.rows = 24;
      addon.handleObservedResize({ width: 809, height: 128 });
      addon._flushFit();
      addon._flushBackendSync();

      assert.deepEqual(
        snapshots.map(({ cols, rows, widthPx, heightPx }) => ({ cols, rows, widthPx, heightPx })),
        [{ cols: 80, rows: 24, widthPx: 809, heightPx: 128 }],
      );
      addon.reset();
    } finally {
      if (previousRequestAnimationFrame === undefined) delete globalThis.requestAnimationFrame;
      else globalThis.requestAnimationFrame = previousRequestAnimationFrame;
      if (previousCancelAnimationFrame === undefined) delete globalThis.cancelAnimationFrame;
      else globalThis.cancelAnimationFrame = previousCancelAnimationFrame;
    }
  });
});

test("fit before activation sync uses the visible mount geometry", () => {
  withWindow(() => {
    const snapshots = [];
    const { addon, terminal } = createAddon({
      onBackendResize: (snapshot) => {
        snapshots.push(snapshot);
        return true;
      },
    });

    addon._lastProposedGeometry = { cols: 0, rows: 0 };
    terminal.cols = 1;
    terminal.rows = 1;
    addon.fitIfNeeded();
    addon.queueBackendSync(null, { immediate: true });

    assert.deepEqual(terminal.resizeCalls, [{ cols: 9, rows: 8 }]);
    assert.deepEqual(
      snapshots.map(({ cols, rows }) => ({ cols, rows })),
      [{ cols: 9, rows: 8 }],
    );
  });
});
