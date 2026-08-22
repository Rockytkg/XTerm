import assert from "node:assert/strict";
import test from "node:test";
import { TerminalHighlightAddon } from "../src/utils/terminal/addons/highlight/TerminalHighlightAddon.js";
import { compileTerminalHighlightRules } from "../src/utils/terminal/highlightRules.js";

function createDisposable(dispose) {
  return { dispose };
}

function createTerminalHarness(initialText = "ERROR") {
  let lines = Array.isArray(initialText) ? [...initialText] : [initialText];
  let writeParsed;
  let scroll;
  let decorationDisposeCount = 0;
  const registeredDecorations = [];
  const registeredMarkers = [];
  const terminal = {
    cols: 32,
    rows: 1,
    buffer: {
      active: {
        baseY: 0,
        cursorY: 0,
        length: lines.length,
        viewportY: 0,
        getLine: (row) => ({
          getCell: (column) => ({
            getChars: () => lines[row]?.[column] || " ",
            getWidth: () => 1,
          }),
          translateToString: () => lines[row] || "",
        }),
      },
    },
    onRender: () => createDisposable(() => {}),
    onResize: () => createDisposable(() => {}),
    onScroll: (handler) => {
      scroll = handler;
      return createDisposable(() => {
        if (scroll === handler) scroll = null;
      });
    },
    onWriteParsed: (handler) => {
      writeParsed = handler;
      return createDisposable(() => {
        if (writeParsed === handler) writeParsed = null;
      });
    },
    registerDecoration: (options) => {
      registeredDecorations.push(options);
      return createDisposable(() => {
        decorationDisposeCount += 1;
      });
    },
    registerMarker: (offset) => {
      const listeners = new Set();
      const marker = {
        isDisposed: false,
        line: terminal.buffer.active.baseY + terminal.buffer.active.cursorY + offset,
        dispose() {
          if (marker.isDisposed) return;
          marker.isDisposed = true;
          for (const listener of [...listeners]) listener();
        },
        onDispose(listener) {
          listeners.add(listener);
          return createDisposable(() => listeners.delete(listener));
        },
      };
      registeredMarkers.push(marker);
      return marker;
    },
  };

  return {
    emitScroll: () => scroll?.(),
    emitWriteParsed: () => writeParsed?.(),
    get decorationDisposeCount() {
      return decorationDisposeCount;
    },
    registeredDecorations,
    registeredMarkers,
    setText: (value) => {
      lines[0] = value;
    },
    setLines: (value) => {
      lines = [...value];
      terminal.buffer.active.length = lines.length;
    },
    terminal,
  };
}

function installAnimationFrameHarness() {
  const callbacks = new Map();
  let nextId = 1;
  const previousRequest = globalThis.requestAnimationFrame;
  const previousCancel = globalThis.cancelAnimationFrame;
  globalThis.requestAnimationFrame = (callback) => {
    const id = nextId++;
    callbacks.set(id, callback);
    return id;
  };
  globalThis.cancelAnimationFrame = (id) => callbacks.delete(id);
  return {
    flush() {
      while (callbacks.size > 0) {
        const batch = [...callbacks.values()];
        callbacks.clear();
        for (const callback of batch) callback();
      }
    },
    restore() {
      globalThis.requestAnimationFrame = previousRequest;
      globalThis.cancelAnimationFrame = previousCancel;
    },
  };
}

const RULE_SET = {
  hash: "error-rule",
  rules: [
    {
      caseSensitive: true,
      foregroundColor: "#ff0000",
      matchType: "text",
      pattern: "ERROR",
    },
  ],
};

test("parsed output refreshes highlights without an external flush notification", () => {
  const frames = installAnimationFrameHarness();
  try {
    const harness = createTerminalHarness("ERROR one");
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);
    frames.flush();

    harness.setText("prefix ERROR two");
    harness.emitWriteParsed();
    frames.flush();

    assert.equal(harness.decorationDisposeCount, 1);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("scrolling offscreen preserves highlights until their buffer content is removed", () => {
  const frames = installAnimationFrameHarness();
  try {
    const harness = createTerminalHarness(["ERROR first", ...Array(9).fill("plain"), "ERROR last"]);
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);
    frames.flush();

    harness.terminal.buffer.active.viewportY = 10;
    harness.emitScroll();
    frames.flush();

    assert.equal(harness.decorationDisposeCount, 0);
    assert.equal(harness.registeredDecorations.length, 2);

    harness.registeredMarkers[0].dispose();
    assert.equal(harness.decorationDisposeCount, 1);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("a fast scroll replaces stale queued viewport work", () => {
  const frames = installAnimationFrameHarness();
  try {
    const lines = Array(100).fill("plain");
    lines[99] = "ERROR current viewport";
    const harness = createTerminalHarness(lines);
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);

    harness.terminal.buffer.active.viewportY = 99;
    harness.emitScroll();
    frames.flush();

    assert.equal(harness.registeredDecorations.length, 1);
    assert.equal(harness.registeredDecorations[0].marker.line, 99);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("scrollback trimming reindexes a surviving marker without recreating its decoration", () => {
  const frames = installAnimationFrameHarness();
  try {
    const harness = createTerminalHarness([
      "discarded",
      "plain",
      "plain",
      "plain",
      "plain",
      "ERROR survives",
    ]);
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);
    frames.flush();

    harness.setLines(["plain", "plain", "plain", "plain", "ERROR survives"]);
    harness.registeredMarkers[0].line = 4;
    harness.terminal.buffer.active.viewportY = 4;
    harness.emitScroll();
    frames.flush();

    assert.equal(harness.registeredDecorations.length, 1);
    assert.equal(harness.decorationDisposeCount, 0);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("pausing scans for an inactive tab preserves existing decorations", () => {
  const frames = installAnimationFrameHarness();
  try {
    let enabled = true;
    const harness = createTerminalHarness();
    const addon = new TerminalHighlightAddon({ getEnabled: () => enabled });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);
    frames.flush();

    enabled = false;
    addon.setRules(RULE_SET);
    assert.equal(harness.decorationDisposeCount, 0);

    enabled = true;
    addon.setRules(RULE_SET);
    frames.flush();
    assert.equal(harness.decorationDisposeCount, 0);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("removing the configured rules clears stale decorations", () => {
  const frames = installAnimationFrameHarness();
  try {
    const harness = createTerminalHarness();
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(RULE_SET);
    frames.flush();

    addon.setRules({ hash: "disabled", rules: [] });

    assert.equal(harness.decorationDisposeCount, 1);
    addon.dispose();
  } finally {
    frames.restore();
  }
});

test("IPv4 endpoint rule highlights the address and attached port as one span", () => {
  const frames = installAnimationFrameHarness();
  try {
    const compiledRules = compileTerminalHighlightRules([
      {
        caseSensitive: false,
        color: "#40c4e8",
        effect: "foreground",
        matchType: "regex",
        pattern:
          "(?<![A-Za-z0-9.])(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)(?:\\.(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)){3}(?:(?:/(?:3[0-2]|[12]?\\d))|(?::(?:6553[0-5]|655[0-2]\\d|65[0-4]\\d{2}|6[0-4]\\d{3}|[1-5]?\\d{1,4})))?(?![A-Za-z0-9.:])",
      },
    ]);
    const harness = createTerminalHarness("target=172.20.200.254:23; startup negotiation timeout");
    const addon = new TerminalHighlightAddon({ getEnabled: () => true });
    addon.activate(harness.terminal);
    addon.setRules(compiledRules);
    frames.flush();

    const endpointDecoration = harness.registeredDecorations.find(
      (decoration) => decoration.x === 7,
    );
    assert.equal(endpointDecoration?.width, "172.20.200.254:23".length);
    assert.equal(endpointDecoration?.foregroundColor, "#40c4e8");
    addon.dispose();
  } finally {
    frames.restore();
  }
});
