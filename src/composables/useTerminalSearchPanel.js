import { computed, ref, watch } from "vue";

const SEARCH_DECORATIONS = Object.freeze({
  matchBackground: "oklch(0.82 0.14 82)",
  matchBorder: "oklch(0.72 0.15 70)",
  matchOverviewRuler: "oklch(0.72 0.15 70)",
  activeMatchBackground: "oklch(0.62 0.18 258)",
  activeMatchBorder: "oklch(0.53 0.18 260)",
  activeMatchColorOverviewRuler: "oklch(0.53 0.18 260)",
});

export function useTerminalSearchPanel({
  props,
  t,
  focusTerminal,
  isForegroundRuntime,
  getSearchAddon,
}) {
  const searchOpen = ref(false);
  const searchTerm = ref("");
  const searchResult = ref({ resultIndex: -1, resultCount: 0 });

  const searchResultLabel = computed(() => {
    if (!searchTerm.value) return t("terminal.searchIdle");
    if (searchResult.value.resultCount <= 0 || searchResult.value.resultIndex < 0) {
      return t("terminal.searchNoResults");
    }
    return `${searchResult.value.resultIndex + 1}/${searchResult.value.resultCount}`;
  });

  // 与 searchResultLabel 的“无结果”分支同条件，供模板作状态判据，避免比对渲染文案。
  const searchEmpty = computed(
    () =>
      Boolean(searchTerm.value) &&
      (searchResult.value.resultCount <= 0 || searchResult.value.resultIndex < 0),
  );

  function setSearchResults(result) {
    searchResult.value = result;
  }

  function resetSearchState() {
    searchOpen.value = false;
    searchTerm.value = "";
    searchResult.value = { resultIndex: -1, resultCount: 0 };
  }

  function openSearchPanel() {
    if (!getSearchAddon() || !isForegroundRuntime()) return;
    searchOpen.value = true;
    searchTerm.value = "";
  }

  function closeSearchPanel() {
    searchOpen.value = false;
    getSearchAddon()?.clearDecorations();
    focusTerminal();
  }

  function runSearch({ previous = false } = {}) {
    const searchAddon = getSearchAddon();
    if (!searchAddon) return;

    const term = searchTerm.value.trim();
    if (!term) {
      searchAddon.clearDecorations();
      searchResult.value = { resultIndex: -1, resultCount: 0 };
      return;
    }

    const options = {
      decorations: SEARCH_DECORATIONS,
    };
    const found = previous
      ? searchAddon.findPrevious(term, options)
      : searchAddon.findNext(term, options);
    if (!found) searchResult.value = { resultIndex: -1, resultCount: 0 };
  }

  watch(
    () => props.searchOpenToken,
    (value, previousValue) => {
      if (value === previousValue || !props.visible) return;
      openSearchPanel();
    },
  );

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
  };
}
