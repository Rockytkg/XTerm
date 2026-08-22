export function resolveTerminalGeometry(activeTerminalSize) {
  const cols = Number(activeTerminalSize.value?.cols);
  const rows = Number(activeTerminalSize.value?.rows);
  if (!Number.isFinite(cols) || !Number.isFinite(rows) || cols <= 0 || rows <= 0) {
    return {};
  }
  return {
    cols: Math.round(cols),
    rows: Math.round(rows),
  };
}
