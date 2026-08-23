//! 桌面会话环境检测。
//!
//! 前端根据结果选择平台特定的交互路径：Wayland 会话不允许客户端查询全局
//! 光标坐标，也不允许程序主动设置窗口绝对位置，因此独立悬浮窗口式的右键
//! 菜单无法定位，需要降级为主窗口内渲染的 DOM 菜单；WebKitGTK 标识则用于
//! 启用对应的样式兼容层。

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopEnvironment {
    /// 操作系统：windows / macos / linux。
    platform: &'static str,
    /// 显示会话：wayland / x11 / unknown；非 Linux 平台恒为 native。
    session: &'static str,
    /// Webview 引擎：webkitgtk / webview2 / wkwebview。
    webview: &'static str,
}

#[tauri::command]
pub(crate) fn desktop_environment() -> DesktopEnvironment {
    current_desktop_environment()
}

#[cfg(target_os = "linux")]
fn current_desktop_environment() -> DesktopEnvironment {
    DesktopEnvironment {
        platform: "linux",
        session: linux_session_type(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
            std::env::var("DISPLAY").ok().as_deref(),
        ),
        webview: "webkitgtk",
    }
}

#[cfg(windows)]
fn current_desktop_environment() -> DesktopEnvironment {
    DesktopEnvironment {
        platform: "windows",
        session: "native",
        webview: "webview2",
    }
}

#[cfg(target_os = "macos")]
fn current_desktop_environment() -> DesktopEnvironment {
    DesktopEnvironment {
        platform: "macos",
        session: "native",
        webview: "wkwebview",
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn current_desktop_environment() -> DesktopEnvironment {
    DesktopEnvironment {
        platform: "unknown",
        session: "unknown",
        webview: "unknown",
    }
}

/// `XDG_SESSION_TYPE` 是权威来源，但部分显示管理器不设置它；此时
/// `WAYLAND_DISPLAY` / `DISPLAY` 非空可分别佐证 Wayland / X11 会话。
/// 测试需要在非 Linux 平台覆盖该纯函数，因此对 test 构建开放。
#[cfg(any(target_os = "linux", test))]
fn linux_session_type(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    x11_display: Option<&str>,
) -> &'static str {
    match session_type
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("wayland") => "wayland",
        Some("x11") => "x11",
        _ if env_flag_present(wayland_display) => "wayland",
        _ if env_flag_present(x11_display) => "x11",
        _ => "unknown",
    }
}

#[cfg(any(target_os = "linux", test))]
fn env_flag_present(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_session_type_is_authoritative() {
        assert_eq!(linux_session_type(Some("wayland"), None, None), "wayland");
        assert_eq!(linux_session_type(Some("x11"), None, None), "x11");
        // 大小写与空白不影响判定。
        assert_eq!(linux_session_type(Some(" Wayland "), None, None), "wayland");
    }

    #[test]
    fn display_variables_fill_in_when_session_type_is_missing() {
        assert_eq!(linux_session_type(None, Some("wayland-0"), None), "wayland");
        assert_eq!(linux_session_type(Some(""), None, Some(":0")), "x11");
        assert_eq!(linux_session_type(None, None, None), "unknown");
        // tty 等其它取值不算图形会话。
        assert_eq!(linux_session_type(Some("tty"), None, None), "unknown");
    }

    #[test]
    fn session_type_wins_over_display_variables() {
        // XWayland 下 DISPLAY 也会存在，不能让 X11 盖过真实的 Wayland 会话。
        assert_eq!(
            linux_session_type(Some("wayland"), Some("wayland-0"), Some(":0")),
            "wayland"
        );
    }
}
