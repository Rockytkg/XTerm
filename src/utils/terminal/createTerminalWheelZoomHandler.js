import { isPrimaryModifier } from "../platform.js";
import { createWheelAccumulator } from "../wheelAccumulator.js";

// 与 RDP/VNC 桌面共用按像素累积的归一化：精密触控板一次捏合会拆成几十个
// 小 deltaY，逐事件步进会让字号瞬间打满/打穿；100px 约等于一个传统滚轮档位。
const ZOOM_THRESHOLD_PX = 100;
const ZOOM_LINE_HEIGHT_PX = 20;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function createTerminalWheelZoomHandler({
  getFontSize,
  setFontSize,
  minFontSize,
  maxFontSize,
}) {
  const accumulator = createWheelAccumulator({
    thresholdPx: ZOOM_THRESHOLD_PX,
    lineHeightPx: ZOOM_LINE_HEIGHT_PX,
    // direction < 0 为向上滚动（放大字号），与浏览器 deltaY 符号一致。
    send: (direction) => {
      const nextFontSize = clamp(
        getFontSize() + (direction < 0 ? 1 : -1),
        minFontSize,
        maxFontSize,
      );
      if (nextFontSize !== getFontSize()) setFontSize(nextFontSize);
      return true;
    },
  });

  return (event) => {
    if (!isPrimaryModifier(event) || event.deltaY === 0) return;
    if (event.cancelable) event.preventDefault();
    event.stopPropagation();
    accumulator.feed(event);
  };
}
