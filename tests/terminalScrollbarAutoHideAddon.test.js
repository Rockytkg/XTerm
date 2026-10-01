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
    this._listeners.set(type, listener);
  }

  removeEventListener(type, listener) {
    if (this._listeners.get(type) === listener) this._listeners.delete(type);
  }

  dispatch(type) {
    this._listeners.get(type)?.({ type });
  }
}

const HIDDEN_CLASS = "xterm-addon-scrollbar-idle-hidden";

mock.timers.enable({ apis: ["setTimeout", "Date"] });

test("hides the scrollbar after inactivity and reveals it on activity", () => {
  const terminal = {
    element: new FakeElement(),
  };
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

test("does not fail when xterm.js has no root DOM", () => {
  const addon = new TerminalScrollbarAutoHideAddon();
  assert.doesNotThrow(() => addon.activate({ element: null }));
  assert.doesNotThrow(() => addon.dispose());
});
