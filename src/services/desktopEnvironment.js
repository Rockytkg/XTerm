import { invokeIpc } from "./ipc/core";
import { isLinuxUserAgent, isMacUserAgent, isWebKitGtkUserAgent } from "../utils/platform";

let environmentPromise = null;

/**
 * IPC 不可用（纯浏览器 dev / 测试环境）时按 UA 推断，保证调用方总有
 * 结构一致的结果可用。
 */
function platformFromUserAgent(userAgent) {
  if (isMacUserAgent(userAgent)) return "macos";
  if (isLinuxUserAgent(userAgent)) return "linux";
  if (/Windows/.test(userAgent)) return "windows";
  return "unknown";
}

function environmentFromUserAgent() {
  const userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent;
  return {
    platform: platformFromUserAgent(userAgent),
    session: "unknown",
    webview: isWebKitGtkUserAgent(userAgent) ? "webkitgtk" : "unknown",
  };
}

/**
 * 桌面环境信息（平台 / 显示会话 / webview 引擎），由后端按条件编译读取
 * 会话环境变量给出权威结果；进程内缓存一次，菜单等高频路径不重复 IPC。
 */
export function getDesktopEnvironment() {
  if (!environmentPromise) {
    environmentPromise = invokeIpc("desktop_environment").catch(environmentFromUserAgent);
  }
  return environmentPromise;
}
