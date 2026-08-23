import assert from "node:assert/strict";
import test from "node:test";
import {
  isLinuxUserAgent,
  isMacPlatform,
  isMacUserAgent,
  isPrimaryModifier,
  isWebKitGtkUserAgent,
} from "../src/utils/platform.js";
import { createTerminalShortcutHandler } from "../src/utils/terminal/createTerminalShortcutHandler.js";

const MAC_SAFARI_UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
const WINDOWS_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const LINUX_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)";

test("isMacUserAgent detects macOS and iOS user agents", () => {
  assert.equal(isMacUserAgent(MAC_SAFARI_UA), true);
  assert.equal(
    isMacUserAgent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15"),
    true,
  );
  assert.equal(
    isMacUserAgent("Mozilla/5.0 (iPad; CPU OS 17_4 like Mac OS X) AppleWebKit/605.1.15"),
    true,
  );
});

test("isMacUserAgent rejects Windows/Linux/empty user agents", () => {
  assert.equal(isMacUserAgent(WINDOWS_UA), false);
  assert.equal(isMacUserAgent(LINUX_UA), false);
  assert.equal(isMacUserAgent(""), false);
  assert.equal(isMacUserAgent(undefined), false);
});

test("isMacPlatform is safe without a browser navigator", () => {
  // node --test 环境没有 mac navigator（Node 自带的 navigator UA 是 Node.js）。
  assert.equal(isMacPlatform(), false);
});

const WEBKITGTK_UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const LINUX_CHROME_UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

test("isLinuxUserAgent detects desktop Linux and rejects Android/others", () => {
  assert.equal(isLinuxUserAgent(WEBKITGTK_UA), true);
  assert.equal(isLinuxUserAgent(LINUX_CHROME_UA), true);
  assert.equal(
    isLinuxUserAgent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/124.0.0.0"),
    false,
  );
  assert.equal(isLinuxUserAgent(WINDOWS_UA), false);
  assert.equal(isLinuxUserAgent(MAC_SAFARI_UA), false);
  assert.equal(isLinuxUserAgent(""), false);
  assert.equal(isLinuxUserAgent(undefined), false);
});

test("isWebKitGtkUserAgent only matches Linux WebKit without Chromium markers", () => {
  assert.equal(isWebKitGtkUserAgent(WEBKITGTK_UA), true);
  // macOS WKWebView 同样没有 Chrome 标识，必须靠 Linux 限定排除。
  assert.equal(isWebKitGtkUserAgent(MAC_SAFARI_UA), false);
  assert.equal(isWebKitGtkUserAgent(LINUX_CHROME_UA), false);
  assert.equal(isWebKitGtkUserAgent(WINDOWS_UA), false);
  assert.equal(isWebKitGtkUserAgent(""), false);
});

test("isPrimaryModifier follows the platform modifier", () => {
  // 当前（非 mac）环境下认 ctrlKey、不认 metaKey。
  assert.equal(isPrimaryModifier({ ctrlKey: true }), true);
  assert.equal(isPrimaryModifier({ metaKey: true }), false);
  assert.equal(isPrimaryModifier({}), false);
  assert.equal(isPrimaryModifier(null), false);
});

function keydownEvent(key, mods = {}) {
  return {
    type: "keydown",
    key,
    ctrlKey: !!mods.ctrl,
    altKey: !!mods.alt,
    shiftKey: !!mods.shift,
    metaKey: !!mods.meta,
    preventDefault() {},
    stopPropagation() {},
  };
}

function createMacHandler(state) {
  const calls = [];
  const handler = createTerminalShortcutHandler({
    copySelection: () => calls.push("copy"),
    hasSelection: () => state.hasSelection,
    pasteClipboard: () => calls.push("paste"),
    sendInterrupt: () => calls.push("interrupt"),
    canOpenSearch: () => false,
    openSearch: () => calls.push("search"),
    searchShortcut: () => "Ctrl+F",
    isMac: true,
  });
  return { calls, handler };
}

test("mac terminal shortcuts: Cmd+C copies selection, passes through otherwise", () => {
  const state = { hasSelection: true };
  const { calls, handler } = createMacHandler(state);

  assert.equal(handler(keydownEvent("c", { meta: true })), false);
  assert.deepEqual(calls, ["copy"]);

  // 无选中时放行（返回 true）让系统处理。
  state.hasSelection = false;
  assert.equal(handler(keydownEvent("c", { meta: true })), true);
  assert.deepEqual(calls, ["copy"]);
});

test("mac terminal shortcuts: Cmd+V pastes and Cmd+Shift+C copies", () => {
  const state = { hasSelection: false };
  const { calls, handler } = createMacHandler(state);

  assert.equal(handler(keydownEvent("v", { meta: true })), false);
  assert.equal(handler(keydownEvent("c", { meta: true, shift: true })), false);
  assert.deepEqual(calls, ["paste", "copy"]);
});

test("mac terminal shortcuts: Ctrl+C interrupt behavior is unchanged", () => {
  const state = { hasSelection: false };
  const { calls, handler } = createMacHandler(state);

  assert.equal(handler(keydownEvent("c", { ctrl: true })), false);
  assert.deepEqual(calls, ["interrupt"]);

  state.hasSelection = true;
  assert.equal(handler(keydownEvent("c", { ctrl: true })), false);
  assert.deepEqual(calls, ["interrupt", "copy"]);
});

test("non-mac handler ignores Cmd shortcuts", () => {
  const calls = [];
  const handler = createTerminalShortcutHandler({
    copySelection: () => calls.push("copy"),
    hasSelection: () => true,
    pasteClipboard: () => calls.push("paste"),
    sendInterrupt: () => calls.push("interrupt"),
    canOpenSearch: () => false,
    openSearch: () => calls.push("search"),
    searchShortcut: () => "Ctrl+F",
    isMac: false,
  });

  assert.equal(handler(keydownEvent("c", { meta: true })), true);
  assert.equal(handler(keydownEvent("v", { meta: true })), true);
  assert.deepEqual(calls, []);
});
