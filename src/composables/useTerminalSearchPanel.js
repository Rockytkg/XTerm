import { computed, nextTick, ref, watch } from "vue";

const SEARCH_DECORATIONS = Object.freeze({
  matchBackground: "oklch(0.82 0.14 82)",
  matchBorder: "oklch(0.72 0.15 70)",
  matchOverviewRuler: "oklch(0.72 0.15 70)",
  activeMatchBackground: "oklch(0.62 0.18 258)",
  activeMatchBorder: "oklch(0.53 0.18 260)",
  activeMatchColorOverviewRuler: "oklch(0.53 0.18 260)",
});

const EMPTY_SEARCH_RESULT = Object.freeze({ resultIndex: -1, resultCount: 0 });

export function useTerminalSearchPanel({
  props,
  t,
  focusTerminal,
  focusSearchInput,
  isForegroundRuntime,
  getSearchAddon,
  clearTerminalSelection,
}) {
  const searchOpen = ref(false);
  const searchTerm = ref("");
  const searchResult = ref(EMPTY_SEARCH_RESULT);
  let handledSearchOpenToken = 0;
  let searchQueryActive = false;

  const searchResultLabel = computed(() => {
    if (!searchTerm.value) return t("terminal.searchIdle");
    if (searchResult.value.resultCount <= 0) {
      return t("terminal.searchNoResults");
    }
    if (searchResult.value.resultIndex < 0) {
      return t("terminal.searchResultLimit", { count: searchResult.value.resultCount });
    }
    return `${searchResult.value.resultIndex + 1}/${searchResult.value.resultCount}`;
  });

  // 与 searchResultLabel 的“无结果”分支同条件，供模板作状态判据，避免比对渲染文案。
  const searchEmpty = computed(
    () => Boolean(searchTerm.value) && searchResult.value.resultCount <= 0,
  );

  function setSearchResults(result) {
    // SearchAddon may finish an incremental highlight after the panel has
    // already been closed. Such a result belongs to the old query and must
    // not repopulate the closed panel's state.
    if (!searchOpen.value || !searchQueryActive || !searchTerm.value) return;
    searchResult.value = result;
  }

  // 清除搜索在终端上留下的全部视觉痕迹：匹配高亮 decoration，以及 addon 为
  // 当前命中项设置的终端选区（clearDecorations 不清选区，不清就会残留一处
  // “高亮”）。addon-search 的补丁让 clearDecorations 同时取消内部增量重排
  // 定时器，关闭后不会再有一次迟到的重排把旧词高亮画回来。
  function clearSearchPaint() {
    getSearchAddon()?.clearDecorations();
    clearTerminalSelection();
  }

  function resetSearchResult() {
    searchResult.value = EMPTY_SEARCH_RESULT;
  }

  watch(
    searchTerm,
    () => {
      // 输入词变化后，旧词的高亮与命中选区立即失效。立即清除而不是等
      // debounced 搜索覆盖：否则在 debounce 窗口内（以及 addon 在终端输出后
      // 用缓存旧词做增量重排时）屏幕上会残留与输入框不符的中间词高亮。
      searchQueryActive = false;
      resetSearchResult();
      clearSearchPaint();
    },
    { flush: "sync" },
  );

  function resetSearchState() {
    searchOpen.value = false;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    clearSearchPaint();
  }

  function openSearchPanel() {
    const searchAddon = getSearchAddon();
    if (!searchAddon || !isForegroundRuntime()) return false;
    if (searchOpen.value) {
      // 面板已打开时重复触发（再次 Ctrl+F / 菜单）：只把焦点带回输入框，
      // 不清空用户正在使用的查询词。
      focusSearchInput?.();
      return true;
    }
    // 只清 decoration：用户在打开搜索前手动选择的文本不应被清掉。
    searchAddon.clearDecorations();
    searchOpen.value = true;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    nextTick(() => focusSearchInput?.());
    return true;
  }

  function closeSearchPanel() {
    searchOpen.value = false;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    clearSearchPaint();
    focusTerminal();
  }

  function runSearch({ previous = false } = {}) {
    const term = searchTerm.value;
    if (!term) {
      // 空查询没有可执行搜索；高亮与选区已由 searchTerm 的 watch 同步清除。
      searchQueryActive = false;
      resetSearchResult();
      return;
    }

    const searchAddon = getSearchAddon();
    if (!searchAddon) return;

    searchQueryActive = true;
    const options = {
      decorations: SEARCH_DECORATIONS,
    };
    if (previous) searchAddon.findPrevious(term, options);
    else searchAddon.findNext(term, options);
  }

  function syncSearchOpenRequest() {
    const token = Number(props.searchOpenToken) || 0;
    if (!token || token === handledSearchOpenToken) return false;
    if (!props.visible) {
      // 打开请求是一次性广播，只应由当前可见的终端消费；隐藏面板若延后
      // 消费，之后每切换到一个 tab 都会各弹一次搜索框。
      handledSearchOpenToken = token;
      return false;
    }
    if (!openSearchPanel()) return false;
    handledSearchOpenToken = token;
    return true;
  }

  watch(() => [props.searchOpenToken, props.visible], syncSearchOpenRequest, { immediate: true });

  return {
    closeSearchPanel,
    openSearchPanel,
    resetSearchState,
    runSearch,
    searchEmpty,
    searchOpen,
    searchResultLabel,
    searchTerm,
    setSearchResults,
    syncSearchOpenRequest,
  };
}
