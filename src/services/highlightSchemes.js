import { invokeIpc } from "./ipc/core";

export function importTerminalHighlightSchemes() {
  return invokeIpc("terminal_highlight_schemes_import");
}

export function exportTerminalHighlightScheme(schemeId) {
  return invokeIpc("terminal_highlight_schemes_export", { schemeId });
}
