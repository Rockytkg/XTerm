import assert from "node:assert/strict";
import test from "node:test";
import { createShortcutRegistry } from "../src/utils/shortcutRegistry.js";

function keydownEvent({ key, ctrlKey = false, shiftKey = false, altKey = false, metaKey = false }) {
  return {
    type: "keydown",
    key,
    code: key === " " ? "Space" : key,
    ctrlKey,
    shiftKey,
    altKey,
    metaKey,
    defaultPrevented: false,
    propagationStopped: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
    stopPropagation() {
      this.propagationStopped = true;
    },
  };
}

test("matched shortcut runs the handler and consumes the event", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({
    id: "copy",
    shortcut: "Ctrl+Shift+C",
    stopPropagation: true,
    run: () => calls.push("copy"),
  });

  const event = keydownEvent({ key: "c", ctrlKey: true, shiftKey: true });
  assert.equal(registry.handleEvent(event), false);
  assert.deepEqual(calls, ["copy"]);
  assert.equal(event.defaultPrevented, true);
  assert.equal(event.propagationStopped, true);
});

test("unmatched events continue without side effects", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({ id: "copy", shortcut: "Ctrl+Shift+C", run: () => calls.push("copy") });

  const event = keydownEvent({ key: "v", ctrlKey: true });
  assert.equal(registry.handleEvent(event), true);
  assert.deepEqual(calls, []);
  assert.equal(event.defaultPrevented, false);
});

test("when-guard declines the match and keeps the event flowing", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({
    id: "search",
    shortcut: () => "Ctrl+F",
    when: () => false,
    run: () => calls.push("search"),
  });

  const event = keydownEvent({ key: "f", ctrlKey: true });
  assert.equal(registry.handleEvent(event), true);
  assert.deepEqual(calls, []);
});

test("dynamic shortcut getters are evaluated per event", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  let shortcut = "Ctrl+F";
  registry.register({
    id: "search",
    shortcut: () => shortcut,
    run: () => calls.push("search"),
  });

  assert.equal(registry.handleEvent(keydownEvent({ key: "f", ctrlKey: true })), false);
  shortcut = "Ctrl+G";
  assert.equal(registry.handleEvent(keydownEvent({ key: "f", ctrlKey: true })), true);
  assert.equal(registry.handleEvent(keydownEvent({ key: "g", ctrlKey: true })), false);
  assert.deepEqual(calls, ["search", "search"]);
});

test("empty shortcut (removed default) never matches", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({ id: "devtools", shortcut: () => "", run: () => calls.push("devtools") });

  assert.equal(registry.handleEvent(keydownEvent({ key: "F12" })), true);
  assert.deepEqual(calls, []);
});

test("disabled contexts do not match until enabled", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({
    id: "terminal.copy",
    context: "terminal",
    shortcut: "Ctrl+Shift+C",
    run: () => calls.push("copy"),
  });

  const event = keydownEvent({ key: "c", ctrlKey: true, shiftKey: true });
  assert.equal(registry.handleEvent(event), true);

  registry.enableContext("terminal");
  assert.equal(registry.handleEvent(event), false);
  assert.deepEqual(calls, ["copy"]);

  registry.disableContext("terminal");
  assert.equal(
    registry.handleEvent(keydownEvent({ key: "c", ctrlKey: true, shiftKey: true })),
    true,
  );
});

test("consume:false runs the handler but lets the event continue", () => {
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({
    id: "escape",
    shortcut: "Escape",
    preventDefault: false,
    consume: false,
    run: () => calls.push("escape"),
  });

  const event = keydownEvent({ key: "Escape" });
  assert.equal(registry.handleEvent(event), true);
  assert.deepEqual(calls, ["escape"]);
  assert.equal(event.defaultPrevented, false);
});

test("attach/detach wires a single keydown listener to the target", () => {
  const listeners = new Map();
  const target = {
    addEventListener: (type, listener) => listeners.set(type, listener),
    removeEventListener: (type) => listeners.delete(type),
  };
  const registry = createShortcutRegistry();
  const calls = [];
  registry.register({ id: "copy", shortcut: "Ctrl+Shift+C", run: () => calls.push("copy") });

  registry.attach(target);
  registry.attach(target);
  assert.equal(listeners.size, 1);

  listeners.get("keydown")(keydownEvent({ key: "c", ctrlKey: true, shiftKey: true }));
  assert.deepEqual(calls, ["copy"]);

  registry.detach();
  assert.equal(listeners.size, 0);
});
