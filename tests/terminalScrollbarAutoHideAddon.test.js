import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { TerminalScrollbarAutoHideAddon } from "../src/utils/terminal/addons/scrollbar/TerminalScrollbarAutoHideAddon.js";

class FakeClassList {
  constructor() {
    this._classes = new Set();
  }

  add(className) {
    this._classes.add(className);
  }

  remove(className) {
    this._classes.delete(className);
  }

  contains(className) {
    return this._classes.has(className);
  }
}

class FakeElement {
  constructor() {
    this.classList = new FakeClassList();
    this._listeners = new Map();
  }

  addEventListener(type, listener) {
    if (!this._listeners.has(type)) this._listeners.set(type, new Set());
    this._listeners.get(type).add(listener);
  }

  removeEventListener(type, listener) {
    this._listeners.get(type)?.delete(listener);
  }

  dispatch(type, props = {}) {
    for (const listener of this._listeners.get(type) ?? []) {
      listener({ type, ...props });
    }
  }
}

function createFakeTerminal() {
  const element = new FakeElement();
  const terminal = { element, _scrollListener: null };
  terminal.onScroll = (listener) => {
    terminal._scrollListener = listener;
    return {
      dispose() {
        terminal._scrollListener = null;
      },
    };
  };
  return terminal;
}

function withPointerEvents(run) {
  const previous = globalThis.PointerEvent;
  globalThis.PointerEvent = class PointerEvent {};
  try {
    run();
  } finally {
    if (previous === undefined) {
      delete globalThis.PointerEvent;
    } else {
      globalThis.PointerEvent = previous;
    }
  }
}

const HIDDEN_CLASS = "xterm-addon-scrollbar-idle-hidden";

mock.timers.enable({ apis: ["setTimeout", "Date"] });

test("hides the scrollbar after inactivity and reveals it on activity", () => {
  const terminal = createFakeTerminal();
  const terminalElement = terminal.element;
  const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

  addon.activate(terminal);
  mock.timers.tick(999);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  mock.timers.tick(1);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  // xterm.js may replace the scrollbar node's className while rendering; the
  // addon state survives because it lives on the terminal root.
  terminalElement.dispatch("keydown");
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  terminalElement.dispatch("mousemove");
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  addon.dispose();
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);
});

test("keeps the scrollbar visible while a press is held and hides after release", () => {
  const terminal = createFakeTerminal();
  const terminalElement = terminal.element;
  const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

  addon.activate(terminal);
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  terminalElement.dispatch("mousedown");
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  // A stationary drag fires no activity; the scrollbar must stay visible.
  mock.timers.tick(5000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  terminalElement.dispatch("mouseup");
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  addon.dispose();
});

test("tracks pointer presses per pointerId and honors pointercancel", () => {
  withPointerEvents(() => {
    const terminal = createFakeTerminal();
    const terminalElement = terminal.element;
    const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

    addon.activate(terminal);
    terminalElement.dispatch("pointerdown", { pointerId: 1 });
    terminalElement.dispatch("pointerdown", { pointerId: 2 });
    mock.timers.tick(5000);
    assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

    // One pointer released, another still held: still not idle.
    terminalElement.dispatch("pointerup", { pointerId: 1 });
    mock.timers.tick(5000);
    assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

    terminalElement.dispatch("pointercancel", { pointerId: 2 });
    mock.timers.tick(1000);
    assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

    addon.dispose();
  });
});

test("reconciles a missed release when the pointer moves without buttons", () => {
  const terminal = createFakeTerminal();
  const terminalElement = terminal.element;
  const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

  addon.activate(terminal);
  terminalElement.dispatch("mousedown");
  terminalElement.dispatch("mousemove", { buttons: 1 });
  mock.timers.tick(5000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  // Released outside the window: no mouseup reaches the addon, but the next
  // move with buttons === 0 ends the press and re-arms the idle hide.
  terminalElement.dispatch("mousemove", { buttons: 0 });
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  addon.dispose();
});

test("window blur ends an in-flight press", () => {
  const terminal = createFakeTerminal();
  const terminalElement = terminal.element;
  const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

  addon.activate(terminal);
  terminalElement.dispatch("mousedown");
  mock.timers.tick(5000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  terminalElement.dispatch("blur");
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  addon.dispose();
});

test("reveals the scrollbar when the terminal content scrolls", () => {
  const terminal = createFakeTerminal();
  const terminalElement = terminal.element;
  const addon = new TerminalScrollbarAutoHideAddon({ idleTimeout: 1000 });

  addon.activate(terminal);
  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  terminal._scrollListener?.(10);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), false);

  mock.timers.tick(1000);
  assert.equal(terminalElement.classList.contains(HIDDEN_CLASS), true);

  addon.dispose();
  assert.equal(terminal._scrollListener, null);
});

test("does not fail when xterm.js has no root DOM", () => {
  const addon = new TerminalScrollbarAutoHideAddon();
  assert.doesNotThrow(() => addon.activate({ element: null }));
  assert.doesNotThrow(() => addon.dispose());
});
