export function createTerminalContextMenuItems({
  t,
  hasSelection,
  canPaste,
  hasSearch,
  copySelection,
  pasteClipboard,
  selectAll,
  clearOutput,
  openSearch,
}) {
  return [
    menuItem("global-cut", t("contextMenu.cut"), hasSelection, copySelection, {
      icon: "cut",
    }),
    menuItem("global-copy", t("contextMenu.copy"), hasSelection, copySelection, {
      icon: "copy",
      shortcut: "Ctrl+Shift+C",
    }),
    menuItem("global-paste", t("contextMenu.paste"), canPaste, pasteClipboard, {
      icon: "paste",
      shortcut: "Ctrl+Shift+V",
    }),
    separator(),
    menuItem("terminal-select-all", t("contextMenu.selectAll"), true, selectAll, {
      icon: "terminalSelectAll",
      shortcut: "Ctrl+A",
    }),
    menuItem("terminal-clear", t("terminal.clearOutput"), true, clearOutput, {
      icon: "clear",
    }),
    menuItem("terminal-search", t("terminal.searchPrompt"), hasSearch, openSearch, {
      icon: "search",
    }),
  ];
}

function menuItem(id, label, enabled, action, options = {}) {
  return {
    id,
    label,
    enabled,
    action,
    ...options,
  };
}

function separator() {
  return { type: "separator" };
}
