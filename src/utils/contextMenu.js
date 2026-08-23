export const CONTEXT_MENU_WINDOW_LABEL = "context-menu";

export const CONTEXT_MENU_EVENTS = Object.freeze({
  action: "xterm-context-menu-action",
  close: "xterm-context-menu-close",
  open: "xterm-context-menu-open",
  ready: "xterm-context-menu-ready",
});

export const CONTEXT_MENU_LAYOUT = Object.freeze({
  itemHeight: 32,
  maxHeight: 540,
  minHeight: 44,
  screenMargin: 8,
  separatorHeight: 9,
  verticalPadding: 12,
  width: 179,
});

/**
 * 右键菜单的后端形态决策。
 *
 * 菜单默认渲染在独立的边框透明悬浮窗口里，靠全局光标坐标定位；Wayland
 * 会话出于安全设计不允许客户端查询全局光标位置，也不允许程序主动设置
 * 窗口的绝对位置（winit/Tauri 的 setPosition 在 Wayland 下是无操作），
 * 悬浮窗口因此无法跟随鼠标，必须降级为主窗口内渲染的 DOM 菜单。
 * X11 以及 Windows/macOS 无此限制，继续使用悬浮窗口。
 */
export function shouldUseDomContextMenu(environment) {
  return environment?.platform === "linux" && environment?.session === "wayland";
}

/** 危险（破坏性）菜单项的统一判定，供悬浮窗口菜单与 DOM 降级菜单共用。 */
export function isContextMenuDangerItem(item) {
  return !!(item?.tone === "danger" || item?.id?.includes("delete"));
}
