// 高精度触摸板滚轮归一化（RDP/VNC 桌面共用）：精密触摸板把一次滚动拆成大量
// 小 deltaY 事件，直接按事件转发会让远端收到海量滚动或丢量。按 deltaMode 归一化
// 为像素后累积，攒够阈值才回调一次 send(direction)；direction 正 = 浏览器 deltaY
// 向下。send 返回 false 时清零余量并停止（发送失败丢余量，不再补发）。
const DEFAULT_LINE_HEIGHT_PX = 19;
const DEFAULT_THRESHOLD_PX = 100;
const DEFAULT_PAGE_HEIGHT_PX = 600;

export function createWheelAccumulator({
  send,
  lineHeightPx = DEFAULT_LINE_HEIGHT_PX,
  thresholdPx = DEFAULT_THRESHOLD_PX,
  getPageHeightPx = () => DEFAULT_PAGE_HEIGHT_PX,
} = {}) {
  let accumulator = 0;

  function normalizeDeltaY(event) {
    if (event.deltaMode === 1) return event.deltaY * lineHeightPx;
    // page 模式按容器可视高度折算。
    if (event.deltaMode === 2) return event.deltaY * getPageHeightPx();
    return event.deltaY;
  }

  function feed(event) {
    const delta = normalizeDeltaY(event);
    // 方向反转时丢掉旧余量，避免反向滚动先抵消残留造成的顿挫。
    if (accumulator !== 0 && Math.sign(delta) !== Math.sign(accumulator)) {
      accumulator = 0;
    }
    accumulator += delta;
    while (Math.abs(accumulator) >= thresholdPx) {
      const direction = accumulator > 0 ? 1 : -1;
      if (!send(direction, event)) {
        accumulator = 0;
        return;
      }
      accumulator -= direction * thresholdPx;
    }
  }

  function reset() {
    accumulator = 0;
  }

  return { feed, reset };
}
