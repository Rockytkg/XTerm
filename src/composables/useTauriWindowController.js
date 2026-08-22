import { onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.window.controller");

export function useTauriWindowController() {
  const isWindowMaximized = ref(false);
  let shellWindow = null;
  let maximizeRefreshTimer = null;
  let unlistenResize = null;
  let disposed = false;

  function getShellWindow() {
    if (shellWindow) return shellWindow;
    try {
      shellWindow = getCurrentWindow();
      return shellWindow;
    } catch (error) {
      logger.error("window.access.failed", error);
      return null;
    }
  }

  async function refreshWindowMaximized() {
    const currentWindow = getShellWindow();
    if (!currentWindow) return;

    try {
      isWindowMaximized.value = await currentWindow.isMaximized();
    } catch (error) {
      logger.error("window.maximized-state.read.failed", error);
    }
  }

  function minimizeWindow() {
    const currentWindow = getShellWindow();
    if (!currentWindow) return;
    currentWindow.minimize().catch((error) => {
      logger.error("window.minimize.failed", error);
    });
  }

  function toggleWindowMaximize() {
    const currentWindow = getShellWindow();
    if (!currentWindow) return;
    currentWindow
      .toggleMaximize()
      .catch((error) => {
        logger.error("window.maximize.toggle.failed", error);
      })
      .finally(() => {
        clearTimeout(maximizeRefreshTimer);
        maximizeRefreshTimer = setTimeout(() => {
          refreshWindowMaximized();
        }, 180);
      });
  }

  function closeWindow() {
    const currentWindow = getShellWindow();
    if (!currentWindow) return;
    currentWindow.close().catch((error) => {
      logger.error("window.close.failed", error);
    });
  }

  onMounted(async () => {
    const currentWindow = getShellWindow();
    if (!currentWindow) return;

    await refreshWindowMaximized();
    // OS 途径（Win+方向键、拖拽吸附）的最大化/还原不经过 toggleWindowMaximize，
    // 监听 resize 保持标题栏按钮状态同步；防抖避免拖拽缩放期间密集 IPC。
    try {
      const unlisten = await currentWindow.onResized(() => {
        clearTimeout(maximizeRefreshTimer);
        maximizeRefreshTimer = setTimeout(() => {
          void refreshWindowMaximized();
        }, 150);
      });
      if (disposed) {
        unlisten();
        return;
      }
      unlistenResize = unlisten;
    } catch (error) {
      logger.error("window.resize.subscribe.failed", error);
    }
  });

  onBeforeUnmount(() => {
    disposed = true;
    clearTimeout(maximizeRefreshTimer);
    unlistenResize?.();
    unlistenResize = null;
  });

  return {
    closeWindow,
    isWindowMaximized,
    minimizeWindow,
    toggleWindowMaximize,
  };
}
