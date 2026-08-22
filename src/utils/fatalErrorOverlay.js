/**
 * 致命错误浮层：不依赖 Vue / i18n / Pinia，保证渲染树崩溃后仍能给出反馈。
 * 文案固定中英双语，避免在 i18n 未初始化或已损坏时再次失败。
 * 同一页面生命周期内只叠加一次；之后的错误仅追加到详情里。
 */

const OVERLAY_ID = "fatal-error-overlay";

function text(tag, content, style) {
  const element = document.createElement(tag);
  element.textContent = content;
  Object.assign(element.style, style);
  return element;
}

export function showFatalErrorOverlay(detail = "") {
  if (typeof document === "undefined" || !document.body) return;
  const existing = document.getElementById(OVERLAY_ID);
  if (existing) {
    const details = existing.querySelector("[data-fatal-details]");
    if (details && detail) details.textContent = `${details.textContent}\n${detail}`.trim();
    return;
  }

  const overlay = document.createElement("div");
  overlay.id = OVERLAY_ID;
  Object.assign(overlay.style, {
    position: "fixed",
    inset: "0",
    zIndex: "2147483647",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    background: "rgba(12, 14, 18, 0.82)",
    fontFamily: "system-ui, sans-serif",
  });

  const card = document.createElement("div");
  Object.assign(card.style, {
    maxWidth: "420px",
    padding: "24px 28px",
    borderRadius: "10px",
    background: "#1c1f26",
    color: "#e6e8eb",
    boxShadow: "0 12px 40px rgba(0, 0, 0, 0.45)",
    textAlign: "center",
  });

  card.appendChild(
    text("h2", "应用发生严重错误 / The application ran into a fatal error", {
      margin: "0 0 10px",
      fontSize: "16px",
      fontWeight: "600",
    }),
  );
  card.appendChild(
    text("p", "界面可能无法继续响应，请重新加载。 / The UI may no longer respond. Please reload.", {
      margin: "0 0 16px",
      fontSize: "13px",
      lineHeight: "1.6",
      opacity: "0.85",
    }),
  );

  if (detail) {
    const details = text("pre", String(detail).slice(0, 600), {
      margin: "0 0 16px",
      padding: "8px 10px",
      maxHeight: "120px",
      overflow: "auto",
      borderRadius: "6px",
      background: "rgba(255, 255, 255, 0.06)",
      fontSize: "11px",
      textAlign: "left",
      whiteSpace: "pre-wrap",
      wordBreak: "break-all",
    });
    details.dataset.fatalDetails = "1";
    card.appendChild(details);
  }

  const reloadButton = text("button", "重新加载 / Reload", {
    padding: "8px 22px",
    border: "none",
    borderRadius: "6px",
    background: "#3b82f6",
    color: "#fff",
    fontSize: "13px",
    cursor: "pointer",
  });
  reloadButton.addEventListener("click", () => window.location.reload());
  card.appendChild(reloadButton);

  overlay.appendChild(card);
  document.body.appendChild(overlay);
}
