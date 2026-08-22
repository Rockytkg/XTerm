/**
 * 后台 tab 挂起调度器（纯逻辑，可单测）。
 *
 * 与 WebGL 30s dispose 节奏对齐：tab 转入后台 delayMs 后触发 suspend
 * （detach channel，后端会话与 replay cache 继续运行）；回前台立即 resume。
 * 快速来回切换时 suspend 计时会被取消；attach/detach 本身的串行化由
 * TerminalSessionRuntimeController 的 transitionChain 保证，这里只产生决策。
 */

const TERMINAL_BACKGROUND_SUSPEND_DELAY_MS = 30_000;

export function createBackgroundSessionSuspender({
  delayMs = TERMINAL_BACKGROUND_SUSPEND_DELAY_MS,
  isBackground,
  suspend,
  resume,
  setTimer = (callback, ms) => setTimeout(callback, ms),
  clearTimer = (handle) => clearTimeout(handle),
}) {
  let timer = null;
  let suspended = false;
  let disposed = false;

  function cancelTimer() {
    if (timer === null) return;
    clearTimer(timer);
    timer = null;
  }

  function fireSuspend() {
    timer = null;
    if (disposed || suspended) return;
    // 防御：计时期间状态可能未经 sync 同步（例如路由卸载竞态），再确认一次。
    if (!isBackground()) return;
    suspended = true;
    suspend();
  }

  function sync(foreground) {
    if (disposed) return;
    if (foreground) {
      cancelTimer();
      if (suspended) {
        suspended = false;
        resume();
      }
      return;
    }
    if (suspended || timer !== null) return;
    timer = setTimer(fireSuspend, delayMs);
  }

  function dispose() {
    disposed = true;
    cancelTimer();
    suspended = false;
  }

  return {
    sync,
    dispose,
    get suspended() {
      return suspended;
    },
  };
}
