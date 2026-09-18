import assert from "node:assert/strict";
import test from "node:test";
import { eventShortcut, normalizeShortcut } from "../src/utils/shortcuts.js";

test("normalizeShortcut lowercases, aliases and sorts parts", () => {
  assert.equal(normalizeShortcut("Ctrl+Shift+C"), "c+ctrl+shift");
  assert.equal(normalizeShortcut("Command+K"), "k+meta");
  assert.equal(normalizeShortcut(""), "");
  assert.equal(normalizeShortcut(null), "");
});

test("space key is aliased consistently between both directions", () => {
  const event = { key: " ", code: "Space", ctrlKey: true };
  assert.equal(normalizeShortcut("Ctrl+Space"), eventShortcut(event));
});

test("plus as the main key survives normalization", () => {
  assert.equal(normalizeShortcut("+"), "+");
  const event = { key: "+", code: "Equal", ctrlKey: true };
  assert.equal(normalizeShortcut("Ctrl++"), eventShortcut(event));
});

test("function keys normalize by event.key", () => {
  const event = { key: "F12", code: "F12" };
  assert.equal(eventShortcut(event), "f12");
  assert.equal(normalizeShortcut("F12"), eventShortcut(event));
});

test("modifier-only events produce no main key", () => {
  const event = { key: "Control", code: "ControlLeft", ctrlKey: true };
  assert.equal(eventShortcut(event), "ctrl");
});
