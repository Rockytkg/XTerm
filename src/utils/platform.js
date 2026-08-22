/**
 * 平台检测：macOS（Tauri WKWebView）以 Cmd（metaKey）为主修饰键，
 * Windows/Linux 以 Ctrl（ctrlKey）为主。node 测试环境没有 navigator，
 * 所有函数在无 navigator 时按非 mac 处理。
 */

// 抽成可注入 UA 的纯函数便于测试；WKWebView 桌面 UA 含 "Macintosh"。
export function isMacUserAgent(userAgent) {
  return /Mac|iPhone|iPad/.test(String(userAgent || ""));
}

export function isMacPlatform() {
  if (typeof navigator === "undefined") return false;
  const dataPlatform = navigator.userAgentData?.platform;
  if (typeof dataPlatform === "string" && dataPlatform) return dataPlatform === "macOS";
  return isMacUserAgent(navigator.userAgent);
}

// 主修饰键：mac 认 Cmd（metaKey），其余平台认 Ctrl（ctrlKey）。
export function isPrimaryModifier(event) {
  return isMacPlatform() ? !!event?.metaKey : !!event?.ctrlKey;
}
