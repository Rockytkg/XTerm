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

// 桌面 Linux UA；排除 Android（本项目不做移动端，但避免误判）。
export function isLinuxUserAgent(userAgent) {
  const ua = String(userAgent || "");
  return /Linux/.test(ua) && !/Android/.test(ua);
}

// WebKitGTK 是 Tauri 在 Linux 上的 webview 引擎：UA 含 AppleWebKit 但不含
// Chromium 系标识（Chrome/Chromium/Edg）。macOS WKWebView 的 UA 同样无
// Chrome 标识，因此必须先限定 Linux。
export function isWebKitGtkUserAgent(userAgent) {
  const ua = String(userAgent || "");
  return isLinuxUserAgent(ua) && /AppleWebKit/.test(ua) && !/Chrome|Chromium|Edg\//.test(ua);
}

export function isWebKitGtkPlatform() {
  if (typeof navigator === "undefined") return false;
  return isWebKitGtkUserAgent(navigator.userAgent);
}
