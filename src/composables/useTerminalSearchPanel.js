import { computed, ref, watch } from "vue";

const SEARCH_DECORATIONS = Object.freeze({
  matchBackground: "oklch(0.82 0.14 82)",
  matchBorder: "oklch(0.72 0.15 70)",
  matchOverviewRuler: "oklch(0.72 0.15 70)",
  activeMatchBackground: "oklch(0.62 0.18 258)",
  activeMatchBorder: "oklch(0.53 0.18 260)",
  activeMatchColorOverviewRuler: "oklch(0.53 0.18 260)",
});

const EMPTY_SEARCH_RESULT = Object.freeze({ resultIndex: -1, resultCount: 0 });
const SEARCH_CLEANUP_DELAY_MS = 250;

export function useTerminalSearchPanel({
  props,
  t,
  focusTerminal,
  isForegroundRuntime,
  getSearchAddon,
}) {
  const searchOpen = ref(false);
  const searchTerm = ref("");
  const searchResult = ref(EMPTY_SEARCH_RESULT);
  let handledSearchOpenToken = 0;
  let delayedCleanupTimer = 0;
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

  function cancelDelayedCleanup() {
    if (!delayedCleanupTimer) return;
    clearTimeout(delayedCleanupTimer);
    delayedCleanupTimer = 0;
  }

  function clearSearchDecorations() {
    cancelDelayedCleanup();
    const searchAddon = getSearchAddon();
    if (!searchAddon) return;

    searchAddon.clearDecorations();

    // addon-search can have an already queued incremental highlight job. Its
    // public clearDecorations API clears the current decorations but cannot
    // cancel that internal timer, so perform one final cleanup after it has
    // had a chance to run. The identity check prevents clearing a replacement
    // addon after a terminal rebuild.
    delayedCleanupTimer = setTimeout(() => {
      delayedCleanupTimer = 0;
      if (getSearchAddon() === searchAddon && (!searchOpen.value || !searchTerm.value)) {
        searchAddon.clearDecorations();
      }
    }, SEARCH_CLEANUP_DELAY_MS);
  }

  function resetSearchResult() {
    searchResult.value = EMPTY_SEARCH_RESULT;
  }

  watch(
    searchTerm,
    () => {
      // A debounced query has not been executed yet. Ignore result events from
      // the previous query until runSearch activates this one.
      searchQueryActive = false;
      resetSearchResult();
    },
    { flush: "sync" },
  );

  function resetSearchState() {
    searchOpen.value = false;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    clearSearchDecorations();
  }

  function openSearchPanel() {
    const searchAddon = getSearchAddon();
    if (!searchAddon || !isForegroundRuntime()) return false;
    searchAddon.clearDecorations();
    searchOpen.value = true;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    return true;
  }

  function closeSearchPanel() {
    searchOpen.value = false;
    searchTerm.value = "";
    searchQueryActive = false;
    resetSearchResult();
    clearSearchDecorations();
    focusTerminal();
  }

  function runSearch({ previous = false } = {}) {
    const searchAddon = getSearchAddon();
    if (!searchAddon) return;

    const term = searchTerm.value;
    if (!term) {
      searchQueryActive = false;
      searchOpen.value && clearSearchDecorations();
      resetSearchResult();
      return;
    }

    searchQueryActive = true;
    const options = {
      decorations: SEARCH_DECORATIONS,
    };
    if (previous) searchAddon.findPrevious(term, options);
    else searchAddon.findNext(term, options);
  }

  function syncSearchOpenRequest() {
    const token = Number(props.searchOpenToken) || 0;
    if (!token || token === handledSearchOpenToken || !props.visible) return false;
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
