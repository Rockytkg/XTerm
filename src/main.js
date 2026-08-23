import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import FloatingContextMenuWindow from "./components/FloatingContextMenuWindow.vue";
import { i18n } from "./i18n";
import { router } from "./router";
import { initializePreferences } from "./composables/useAppPreferences";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { createLogger } from "./utils/logger";
import { noop } from "./utils/noop";
import { isWebKitGtkUserAgent } from "./utils/platform";
import { getLogLevel } from "./services/logging";
import { showFatalErrorOverlay } from "./utils/fatalErrorOverlay";
import "virtual:uno.css";
import "./styles.scss";

const logger = createLogger("frontend.startup");
const currentWindowLabel = getCurrentWebviewWindow().label;
const currentWindowParam = new URLSearchParams(window.location.search).get("window");
const isContextMenuWindow =
  currentWindowLabel === "context-menu" || currentWindowParam === "context-menu";
const startupSplash = document.getElementById("startup-splash");

document.documentElement.dataset.window = isContextMenuWindow ? "context-menu" : "main";

// WebKitGTK（Linux webview）存在若干渲染/合成差异（如 backdrop-filter 走
// CPU 模糊导致掉帧），尽早打上标记让 compat-webkitgtk.scss 在首帧前生效。
if (isWebKitGtkUserAgent(navigator.userAgent)) {
  document.documentElement.dataset.webview = "webkitgtk";
}

const FONT_LOAD_TIMEOUT_MS = 3000;
const STARTUP_SPLASH_FADE_MS = 320;

function nextAnimationFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

function describeFatalError(error) {
  if (!error) return "";
  if (error instanceof Error) return error.stack || error.message;
  return String(error);
}

let windowErrorHandlersInstalled = false;

/**
 * 全局错误兜底：Vue 渲染/生命周期错误与同步脚本错误视为致命错误，
 * 除日志上报外弹出原生 DOM 浮层提供“重新加载”出口，避免白屏无反馈。
 * 未处理的 Promise 拒绝多为瞬时 IPC 失败，只记录日志，不打断界面。
 */
function installGlobalErrorHandlers(app) {
  app.config.errorHandler = (error, _instance, info) => {
    logger.error("fatal.vue", { error: describeFatalError(error), info });
    showFatalErrorOverlay(describeFatalError(error));
  };
  if (windowErrorHandlersInstalled) return;
  windowErrorHandlersInstalled = true;
  window.addEventListener("error", (event) => {
    logger.error("fatal.window", event.error || event.message);
    showFatalErrorOverlay(describeFatalError(event.error || event.message));
  });
  window.addEventListener("unhandledrejection", (event) => {
    logger.error("unhandledrejection", event.reason);
  });
}

async function waitForDocumentFonts() {
  if (!document.fonts?.ready) return;

  let timeoutId;
  try {
    await Promise.race([
      document.fonts.ready,
      new Promise((resolve) => {
        timeoutId = window.setTimeout(resolve, FONT_LOAD_TIMEOUT_MS);
      }),
    ]);
  } finally {
    window.clearTimeout(timeoutId);
  }
}

async function showMainWindow() {
  try {
    await getCurrentWindow().show();
  } catch (error) {
    logger.error("window.show.failed", error);
  } finally {
    startupSplash?.classList.add("is-fading");
    setTimeout(() => startupSplash?.remove(), STARTUP_SPLASH_FADE_MS);
  }
}

async function mountMainWindow() {
  // 启动即同步后端持久化的日志级别，避免生产模式前端门控与后端漂移；
  // 失败不阻塞启动（保持默认级别）。
  getLogLevel().catch(noop);

  try {
    await initializePreferences();
  } catch (error) {
    logger.error("preferences.initialize.failed", error);
  }

  const app = createApp(App).use(createPinia()).use(i18n).use(router);
  installGlobalErrorHandlers(app);
  app.mount("#app");

  await router.isReady();
  await nextTick();
  await waitForDocumentFonts();
  await nextAnimationFrame();
  await showMainWindow();
}

if (isContextMenuWindow) {
  startupSplash?.remove();
  const contextMenuApp = createApp(FloatingContextMenuWindow).use(i18n);
  installGlobalErrorHandlers(contextMenuApp);
  contextMenuApp.mount("#app");
} else {
  mountMainWindow();
}
