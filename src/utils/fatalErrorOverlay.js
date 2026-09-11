/**
 * 致命错误浮层：不依赖 Vue / i18n / Pinia，保证渲染树崩溃后仍能给出反馈。
 * 样式经内联 <style> 自包含注入，文案固定中英双语，避免 i18n 未初始化或损坏时再次失败。
 * 同一页面生命周期内只叠加一次；之后的错误仅追加到详情里（保留开头，崩溃首帧堆栈最有价值）。
 */

const OVERLAY_ID = "fatal-error-overlay";
const DETAILS_LIMIT = 4000;

const STYLES = `
#${OVERLAY_ID} {
  position: fixed;
  inset: 0;
  z-index: 2147483647;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(10, 12, 16, 0.6);
  backdrop-filter: blur(6px);
  font-family: system-ui, sans-serif;
  animation: feo-fade 0.18s ease-out;
}
#${OVERLAY_ID} .feo-card {
  width: min(440px, calc(100vw - 48px));
  padding: 22px 24px;
  border-radius: 14px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: #20242e;
  background: oklch(23% 0.014 260deg);
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  color: #e6e8eb;
  animation: feo-pop 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.15);
}
#${OVERLAY_ID} .feo-header {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}
#${OVERLAY_ID} .feo-icon {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 10px;
  background: #453031;
  background: oklch(29% 0.04 25deg);
  color: #f0998f;
  color: oklch(82% 0.12 25deg);
}
#${OVERLAY_ID} .feo-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.5;
}
#${OVERLAY_ID} .feo-subtitle {
  margin: 1px 0 0;
  font-size: 12px;
  opacity: 0.55;
}
#${OVERLAY_ID} .feo-message {
  margin: 14px 0 0;
  font-size: 13px;
  line-height: 1.6;
  opacity: 0.85;
}
#${OVERLAY_ID} .feo-details {
  margin: 14px 0 0;
  padding: 10px 12px;
  max-height: 140px;
  overflow: auto;
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.06);
  background: rgba(0, 0, 0, 0.32);
  color: #a8b0bd;
  font-family: ui-monospace, Consolas, monospace;
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  user-select: text;
}
#${OVERLAY_ID} .feo-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 20px;
}
#${OVERLAY_ID} .feo-button {
  padding: 7px 18px;
  border-radius: 8px;
  border: 1px solid transparent;
  font-size: 13px;
  color: inherit;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}
#${OVERLAY_ID} .feo-button-ghost {
  background: transparent;
  border-color: rgba(255, 255, 255, 0.18);
}
#${OVERLAY_ID} .feo-button-ghost:hover {
  background: rgba(255, 255, 255, 0.08);
}
#${OVERLAY_ID} .feo-button-primary {
  background: #3b6fe0;
  background: oklch(58% 0.17 258deg);
  color: #fff;
}
#${OVERLAY_ID} .feo-button-primary:hover {
  background: #2f5fd0;
  background: oklch(53% 0.18 258deg);
}
#${OVERLAY_ID} .feo-button:focus-visible {
  outline: 2px solid #7aa2f7;
  outline-offset: 2px;
}
@keyframes feo-fade {
  from { opacity: 0; }
}
@keyframes feo-pop {
  from { opacity: 0; transform: translateY(8px) scale(0.97); }
}
`;

const ICON_SVG =
  '<svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
  'stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' +
  '<path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>' +
  '<line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';

function h(tag, className, content) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (content) element.textContent = content;
  return element;
}

export function showFatalErrorOverlay(detail = "") {
  if (typeof document === "undefined" || !document.body) return;
  const existing = document.getElementById(OVERLAY_ID);
  if (existing) {
    const details = existing.querySelector("[data-fatal-details]");
    if (details && detail) {
      details.textContent = `${details.textContent}\n${detail}`.trim().slice(0, DETAILS_LIMIT);
    }
    return;
  }

  const overlay = h("div");
  overlay.id = OVERLAY_ID;
  overlay.setAttribute("role", "alertdialog");
  overlay.setAttribute("aria-modal", "true");

  const style = h("style");
  style.textContent = STYLES;
  overlay.appendChild(style);

  const card = h("div", "feo-card");

  const header = h("div", "feo-header");
  const icon = h("span", "feo-icon");
  icon.innerHTML = ICON_SVG;
  const titleBlock = h("div");
  titleBlock.appendChild(h("h2", "feo-title", "应用发生严重错误"));
  titleBlock.appendChild(h("p", "feo-subtitle", "The application ran into a fatal error"));
  header.append(icon, titleBlock);
  card.appendChild(header);

  card.appendChild(
    h(
      "p",
      "feo-message",
      "界面可能无法继续响应，请重新加载。 / The UI may no longer respond. Please reload.",
    ),
  );

  if (detail) {
    const details = h("pre", "feo-details", String(detail).slice(0, DETAILS_LIMIT));
    details.dataset.fatalDetails = "1";
    card.appendChild(details);
  }

  const onKeydown = (event) => {
    if (event.key === "Escape") close();
  };
  function close() {
    document.removeEventListener("keydown", onKeydown, true);
    overlay.remove();
  }
  document.addEventListener("keydown", onKeydown, true);

  const closeButton = h("button", "feo-button feo-button-ghost", "关闭 / Close");
  closeButton.addEventListener("click", close);
  const reloadButton = h("button", "feo-button feo-button-primary", "重新加载 / Reload");
  reloadButton.addEventListener("click", () => window.location.reload());
  const actions = h("div", "feo-actions");
  actions.append(closeButton, reloadButton);
  card.appendChild(actions);

  overlay.appendChild(card);
  document.body.appendChild(overlay);
  closeButton.focus();
}
