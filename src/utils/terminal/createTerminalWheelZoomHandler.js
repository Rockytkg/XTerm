import { isPrimaryModifier } from "../platform.js";

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function createTerminalWheelZoomHandler({
  getFontSize,
  setFontSize,
  minFontSize,
  maxFontSize,
}) {
  return (event) => {
    if (!isPrimaryModifier(event) || event.deltaY === 0) return;
    if (event.cancelable) event.preventDefault();
    event.stopPropagation();
    const nextFontSize = clamp(
      getFontSize() + (event.deltaY < 0 ? 1 : -1),
      minFontSize,
      maxFontSize,
    );
    if (nextFontSize !== getFontSize()) setFontSize(nextFontSize);
  };
}
