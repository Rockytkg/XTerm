import assert from "node:assert/strict";
import test from "node:test";
import { nextTick, reactive } from "vue";
import { useTerminalSearchPanel } from "../src/composables/useTerminalSearchPanel.js";

function createSearchHarness({ token = 0, visible = true, addon = null } = {}) {
  const props = reactive({ searchOpenToken: token, visible });
  const state = {
    clearCalls: 0,
    selectionClears: 0,
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
    clearTerminalSelection: () => {
      state.selectionClears += 1;
    },
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

test("search state cleanup removes decorations, selection and stale result state", () => {
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
  // open(1) + 输入 watch(1) + 关闭时 watch(1) + 关闭时显式清理(1)
  assert.equal(harness.state.clearCalls, 4);
  // 关闭必须清掉 addon 为命中项设置的终端选区，否则最后命中处残留“高亮”
  assert.ok(harness.state.selectionClears > 0);
});

test("an empty query runs no search and leaves no paint behind", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.panel.searchTerm.value = "";

  harness.panel.runSearch();
  assert.equal(harness.state.nextCalls.length, 0);
  assert.equal(harness.panel.searchEmpty.value, false);
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchIdle");
});

test("editing a query immediately clears stale paint before the debounced search runs", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.setResultsListener(harness.panel.setSearchResults);

  harness.panel.searchTerm.value = "first";
  harness.panel.runSearch();
  assert.equal(harness.panel.searchResultLabel.value, "1/1");
  const clearCallsAfterSearch = harness.state.clearCalls;
  const selectionClearsAfterSearch = harness.state.selectionClears;

  // 输入新词但 debounced 搜索尚未执行：旧词高亮与命中选区必须立即消失，
  // 不能等到下一次 findNext 才被覆盖。
  harness.panel.searchTerm.value = "second";
  assert.equal(harness.panel.searchResultLabel.value, "terminal.searchNoResults");
  assert.equal(harness.state.clearCalls, clearCallsAfterSearch + 1);
  assert.equal(harness.state.selectionClears, selectionClearsAfterSearch + 1);
});

test("deleting the query text clears decorations and selection immediately", () => {
  const harness = createSearchHarness();
  harness.panel.openSearchPanel();
  harness.setResultsListener(harness.panel.setSearchResults);

  harness.panel.searchTerm.value = "abc";
  harness.panel.runSearch();
  const clearCallsAfterSearch = harness.state.clearCalls;
  const selectionClearsAfterSearch = harness.state.selectionClears;

  harness.panel.searchTerm.value = "";
  assert.equal(harness.state.clearCalls, clearCallsAfterSearch + 1);
  assert.equal(harness.state.selectionClears, selectionClearsAfterSearch + 1);

  // 空词触发的 runSearch 不再重复清理，也不执行搜索
  harness.panel.runSearch();
  assert.equal(harness.state.clearCalls, clearCallsAfterSearch + 1);
  assert.equal(harness.state.nextCalls.length, 1);
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
    clearTerminalSelection: () => {},
  });

  assert.equal(panel.searchOpen.value, false);

  searchAddon = { clearDecorations() {} };
  assert.equal(panel.syncSearchOpenRequest(), true);
  assert.equal(panel.searchOpen.value, true);
});
