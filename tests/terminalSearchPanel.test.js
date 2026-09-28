import assert from "node:assert/strict";
import test from "node:test";
import { nextTick, reactive } from "vue";
import { useTerminalSearchPanel } from "../src/composables/useTerminalSearchPanel.js";

function createSearchHarness({ token = 0, visible = true, addon = null } = {}) {
  const props = reactive({ searchOpenToken: token, visible });
  const state = {
    clearCalls: 0,
    nextCalls: [],
    previousCalls: [],
  };
  let resultsListener = null;
  const searchAddon = addon || {
    clearDecorations() {
      state.clearCalls += 1;
    },
    findNext(term) {
      state.nextCalls.push(term);
      resultsListener?.({ resultIndex: 0, resultCount: 1 });
      return true;
    },
    findPrevious(term) {
      state.previousCalls.push(term);
      resultsListener?.({ resultIndex: 0, resultCount: 1 });
      return true;
    },
  };
  const panel = useTerminalSearchPanel({
    props,
    t: (key) => key,
    focusTerminal: () => {},
    isForegroundRuntime: () => props.visible,
    getSearchAddon: () => searchAddon,
  });

  return {
    panel,
    props,
    searchAddon,
    state,
    setResultsListener(listener) {
      resultsListener = listener;
    },
  };
}

test("search state cleanup removes decorations and stale result state", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.panel.searchTerm.value = "needle";
  harness.setResultsListener(harness.panel.setSearchResults);

  harness.panel.runSearch();
  assert.equal(harness.panel.searchResultLabel.value, "1/1");

  harness.panel.closeSearchPanel();
  assert.equal(harness.panel.searchOpen.value, false);
  assert.equal(harness.panel.searchTerm.value, "");
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchIdle");
  assert.equal(harness.state.clearCalls, 2);
});

test("an empty query clears the active search", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.panel.searchTerm.value = "";

  harness.panel.runSearch();
  assert.equal(harness.state.nextCalls.length, 0);
  assert.equal(harness.state.clearCalls, 2);
  assert.equal(harness.panel.searchEmpty.value, false);
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchIdle");
});

test("editing a query invalidates the previous result before the debounced search runs", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.setResultsListener(harness.panel.setSearchResults);

  harness.panel.searchTerm.value = "first";
  harness.panel.runSearch();
  assert.equal(harness.panel.searchResultLabel.value, "1/1");

  harness.panel.searchTerm.value = "second";
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchNoResults");
});

test("search preserves whitespace and reports when the addon result limit is reached", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.panel.searchTerm.value = " ";
  harness.setResultsListener(harness.panel.setSearchResults);

  harness.panel.runSearch();

  assert.deepEqual(harness.state.nextCalls, [" "]);
  harness.panel.setSearchResults({ resultIndex: -1, resultCount: 1000 });
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchResultLimit");
  assert.equal(harness.panel.searchEmpty.value, false);
});

test("a search request received while hidden is consumed when the terminal becomes visible", async () => {
  const harness = createSearchHarness({ token: 1, visible: false });
  assert.equal(harness.panel.searchOpen.value, false);

  harness.props.visible = true;
  await nextTick();

  assert.equal(harness.panel.searchOpen.value, true);
});

test("an initial search request can be synchronized after the addon is installed", () => {
  let searchAddon = null;
  const props = reactive({ searchOpenToken: 1, visible: true });
  const panel = useTerminalSearchPanel({
    props,
    t: (key) => key,
    focusTerminal: () => {},
    isForegroundRuntime: () => true,
    getSearchAddon: () => searchAddon,
  });

  assert.equal(panel.searchOpen.value, false);

  searchAddon = { clearDecorations() {} };
  assert.equal(panel.syncSearchOpenRequest(), true);
  assert.equal(panel.searchOpen.value, true);
});
