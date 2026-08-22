export function blurActiveElement(options = {}) {
  const { within, exclude } = options;
  const activeElement = document.activeElement;
  if (!(activeElement instanceof HTMLElement)) return false;
  if (typeof exclude === "function" && exclude(activeElement)) return false;
  if (within && !activeElement.closest?.(within)) return false;
  activeElement.blur();
  return true;
}
