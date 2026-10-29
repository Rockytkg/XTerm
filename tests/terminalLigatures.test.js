import assert from "node:assert/strict";
import test from "node:test";

import { parseFontFamilies } from "../src/services/terminalLigatures.js";
import { TauriLigaturesAddon } from "../src/utils/terminal/addons/ligatures/TauriLigaturesAddon.js";
import { LigatureMatcher } from "../src/utils/terminal/addons/ligatures/ligatureMatcher.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function flushAsyncWork() {
  for (let index = 0; index < 3; index += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function createTerminal(fontFamily = '"Fira Code", monospace') {
  const joiners = new Map();
  const refreshCalls = [];
  let nextJoinerId = 1;
  const terminal = {
    element: { style: { fontFeatureSettings: "initial" } },
    options: { fontFamily },
    rows: 24,
    registerCharacterJoiner(handler) {
      const id = nextJoinerId++;
      joiners.set(id, handler);
      return id;
    },
    deregisterCharacterJoiner(id) {
      joiners.delete(id);
    },
    refresh(start, end) {
      refreshCalls.push([start, end]);
    },
    join(text) {
      return [...joiners.values()][0]?.(text) || [];
    },
    refreshCalls,
    joinerCount: () => joiners.size,
  };
  return terminal;
}

test("ligature matcher prefers the longest non-overlapping sequences", () => {
  const matcher = new LigatureMatcher(["->", "-->", "==", "===", "fi", "="]);
  assert.deepEqual(matcher.findRanges("a-->b===fi"), [
    [1, 4],
    [5, 8],
    [8, 10],
  ]);
  assert.deepEqual(matcher.findRanges("plain text"), []);
});

test("cached ranges stay isolated from xterm and WebGL in-place mutations", () => {
  const matcher = new LigatureMatcher(["->"]);
  const first = matcher.findRanges("x->y");
  first[0][0] = 99;
  first.shift();

  const second = matcher.findRanges("x->y");
  assert.deepEqual(second, [[1, 3]]);
  second[0][1] = 0;
  assert.deepEqual(matcher.findRanges("x->y"), [[1, 3]]);
});

test("ligature matcher bounds its line cache", () => {
  const matcher = new LigatureMatcher(["->"]);
  for (let index = 0; index < 600; index += 1) {
    matcher.findRanges(`${index}->`);
  }
  assert.equal(matcher._cache.size, 512);
});

test("addon swaps fallback matching for the resolved font sequences", async () => {
  const loading = deferred();
  const terminal = createTerminal();
  const addon = new TauriLigaturesAddon({
    fallbackLigatures: ["-->"],
    loadLigatures: () => loading.promise,
  });
  addon.activate(terminal);

  assert.deepEqual(terminal.join("-->"), [[0, 3]]);
  assert.equal(terminal.element.style.fontFeatureSettings, '"calt" on');
  loading.resolve(["fi"]);
  await flushAsyncWork();

  assert.deepEqual(terminal.join("-->"), []);
  assert.deepEqual(terminal.join("fi"), [[0, 2]]);
  assert.deepEqual(terminal.refreshCalls, [[0, 23]]);
  addon.dispose();
});

test("only the latest font family can apply asynchronous ligature results", async () => {
  const requests = new Map();
  const terminal = createTerminal('"Font A"');
  const addon = new TauriLigaturesAddon({
    fallbackLigatures: ["-->"],
    loadLigatures: ({ fontFamily }) => {
      const loading = deferred();
      requests.set(fontFamily, loading);
      return loading.promise;
    },
  });
  addon.activate(terminal);
  await flushAsyncWork();
  terminal.options.fontFamily = '"Font B"';
  terminal.join("-->");
  await flushAsyncWork();

  requests.get('"Font B"').resolve(["=="]);
  await flushAsyncWork();
  requests.get('"Font A"').resolve(["fi"]);
  await flushAsyncWork();

  assert.deepEqual(terminal.join("fi"), []);
  assert.deepEqual(terminal.join("=="), [[0, 2]]);
  assert.equal(terminal.refreshCalls.length, 1);
  addon.dispose();
});

test("an empty resolved sequence set disables unnecessary joins", async () => {
  const terminal = createTerminal();
  const addon = new TauriLigaturesAddon({
    fallbackLigatures: ["-->"],
    loadLigatures: async () => [],
  });
  addon.activate(terminal);
  await flushAsyncWork();

  assert.deepEqual(terminal.join("-->"), []);
  addon.dispose();
});

test("dispose deregisters the joiner, restores styles and ignores late loads", async () => {
  const loading = deferred();
  const terminal = createTerminal();
  const addon = new TauriLigaturesAddon({ loadLigatures: () => loading.promise });
  addon.activate(terminal);
  addon.dispose();

  assert.equal(terminal.joinerCount(), 0);
  assert.equal(terminal.element.style.fontFeatureSettings, "initial");
  loading.resolve(["fi"]);
  await flushAsyncWork();
  assert.equal(terminal.refreshCalls.length, 0);
});

test("activation requires an opened terminal", () => {
  const addon = new TauriLigaturesAddon({ loadLigatures: async () => null });
  assert.throws(() => addon.activate({}), /before open is called/);
});

test("CSS font family parsing preserves quoted commas and escapes", () => {
  assert.deepEqual(parseFontFamilies('"My, Font", Fira\\ Code, monospace,,'), [
    "My, Font",
    "Fira Code",
    "monospace",
  ]);
});
