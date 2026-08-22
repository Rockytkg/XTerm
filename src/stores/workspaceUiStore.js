import { ref } from "vue";
import { defineStore } from "pinia";

export const useWorkspaceUiStore = defineStore("workspaceUi", () => {
  const navExpanded = ref(false);
  const rightSidebarView = ref(null);
  const terminalSearchOpenToken = ref(0);

  function toggleNavExpanded() {
    navExpanded.value = !navExpanded.value;
  }

  function requestTerminalSearch() {
    terminalSearchOpenToken.value += 1;
  }

  return {
    navExpanded,
    rightSidebarView,
    terminalSearchOpenToken,
    toggleNavExpanded,
    requestTerminalSearch,
  };
});
