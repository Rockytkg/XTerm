const WEBGL_INACTIVE_DISPOSE_DELAY_MS = 30_000;
// WebGL context 丢失（GPU 重置、驱动崩溃）后的自动恢复退避。尝试耗尽后保持
// DOM 渲染器；返回前台时的常规 sync 也会重新尝试加载。
const WEBGL_CONTEXT_RECOVERY_DELAYS_MS = [2_000, 5_000, 15_000];

export function useTerminalOptionalAddons({ logger, getContext, loadAddon }) {
  let webglAddon = null;
  let webglLoadPromise = null;
  let webglLoadToken = 0;
  let webglDisposeTimer = undefined;
  let webglRecoveryTimer = undefined;
  let webglRecoveryAttempt = 0;
  let ligaturesAddon = null;
  let ligaturesLoadHandle = null;
  let ligaturesLoadPromise = null;

  function refreshTerminal(terminal) {
    if (!terminal) return;
    terminal.refresh(0, Math.max(0, terminal.rows - 1));
  }

  function disposeOptionalAddons() {
    cancelLigaturesLoad();
    cancelWebglDispose();
    cancelWebglRecovery();
    disposeWebglAddon();
    webglLoadToken += 1;
    webglLoadPromise = null;
    webglRecoveryAttempt = 0;
    ligaturesAddon?.dispose?.();
    ligaturesAddon = null;
    ligaturesLoadPromise = null;
  }

  function cancelWebglDispose() {
    clearTimeout(webglDisposeTimer);
    webglDisposeTimer = undefined;
  }

  function cancelWebglRecovery() {
    clearTimeout(webglRecoveryTimer);
    webglRecoveryTimer = undefined;
  }

  function disposeWebglAddon() {
    webglAddon?.dispose?.();
    webglAddon = null;
  }

  function scheduleWebglDispose() {
    if (!webglAddon || webglDisposeTimer) return;
    webglDisposeTimer = setTimeout(() => {
      webglDisposeTimer = undefined;
      const current = getContext();
      if (!current.isForegroundRuntime) disposeWebglAddon();
    }, WEBGL_INACTIVE_DISPOSE_DELAY_MS);
  }

  function cancelLigaturesLoad() {
    if (!ligaturesLoadHandle) return;
    if (window.cancelIdleCallback) {
      window.cancelIdleCallback(ligaturesLoadHandle);
    } else {
      clearTimeout(ligaturesLoadHandle);
    }
    ligaturesLoadHandle = null;
  }

  async function installLigaturesAddon(generation) {
    const { terminal } = getContext();
    if (!terminal || ligaturesAddon || ligaturesLoadPromise) return ligaturesLoadPromise;

    ligaturesLoadPromise = (async () => {
      const { LigaturesAddon } = await import("@xterm/addon-ligatures");
      const current = getContext();
      if (current.disposed || current.generation !== generation || !current.terminal) return;
      loadAddon(
        "ligatures",
        () => new LigaturesAddon(),
        (addon) => {
          ligaturesAddon = addon;
        },
      );
      // Xterm 6 exposes the renderer-level invalidation API. Rebuilding the
      // addon was required by older releases, but it races with addon loading
      // and can leave a stale canvas frame visible during the hand-off.
      current.terminal.clearTextureAtlas?.();
      refreshTerminal(current.terminal);
    })();

    try {
      await ligaturesLoadPromise;
    } catch (error) {
      logger.warn("Terminal addon 'ligatures' failed to load:", error);
      ligaturesAddon = null;
    } finally {
      ligaturesLoadPromise = null;
    }
  }

  function schedulePostOpenTerminalAddons(generation) {
    cancelLigaturesLoad();
    const load = () => {
      ligaturesLoadHandle = null;
      void installLigaturesAddon(generation);
    };
    if (window.requestIdleCallback) {
      ligaturesLoadHandle = window.requestIdleCallback(load, { timeout: 600 });
    } else {
      ligaturesLoadHandle = setTimeout(load, 80);
    }
  }

  function scheduleWebglRecovery() {
    if (webglRecoveryTimer) return;
    if (webglRecoveryAttempt >= WEBGL_CONTEXT_RECOVERY_DELAYS_MS.length) return;
    webglRecoveryTimer = setTimeout(() => {
      webglRecoveryTimer = undefined;
      const current = getContext();
      // 后台/已关闭的终端不恢复：返回前台时 applyRuntimePresentation 会走常规
      // sync 路径重新加载，无需消耗退避次数。
      if (
        current.disposed ||
        !current.terminal ||
        !current.terminalWebgl ||
        !current.isForegroundRuntime ||
        webglAddon
      ) {
        return;
      }
      webglRecoveryAttempt += 1;
      void Promise.resolve(syncTerminalRenderer(current.generation)).then(() => {
        // 恢复失败（GPU 仍在重置）继续退避；成功时 sync 内已清零计数。
        if (!webglAddon) scheduleWebglRecovery();
      });
    }, WEBGL_CONTEXT_RECOVERY_DELAYS_MS[webglRecoveryAttempt]);
  }

  async function syncTerminalRenderer(generation) {
    const { terminal, isForegroundRuntime, terminalWebgl } = getContext();
    if (!terminal) return;
    if (!terminalWebgl) {
      cancelWebglDispose();
      cancelWebglRecovery();
      disposeWebglAddon();
      webglLoadToken += 1;
      webglLoadPromise = null;
      return;
    }
    if (!isForegroundRuntime) {
      scheduleWebglDispose();
      return;
    }
    cancelWebglDispose();
    if (webglAddon || webglLoadPromise) return webglLoadPromise;

    let loadSkipped = false;
    const loadToken = ++webglLoadToken;
    webglLoadPromise = (async () => {
      const { WebglAddon } = await import("@xterm/addon-webgl");
      const current = getContext();
      if (
        loadToken !== webglLoadToken ||
        current.disposed ||
        current.generation !== generation ||
        !current.terminal ||
        !current.terminalWebgl ||
        !current.isForegroundRuntime
      ) {
        loadSkipped = true;
        return;
      }
      const addon = new WebglAddon({
        customGlyphs: current.terminalCustomGlyphs,
      });
      addon.onContextLoss(() => {
        logger.warn("WebGL context lost, falling back to DOM renderer");
        if (webglAddon === addon) {
          disposeWebglAddon();
          refreshTerminal(current.terminal);
          scheduleWebglRecovery();
        }
      });
      current.terminal.loadAddon(addon);
      webglAddon = addon;
      webglRecoveryAttempt = 0;
      // WebGL is loaded after Terminal.open(). Rebuild the complete viewport so
      // the new renderer does not inherit a stale render state.
      refreshTerminal(current.terminal);
    })();

    try {
      await webglLoadPromise;
    } catch (error) {
      logger.warn("WebGL renderer failed to load:", error);
      if (loadToken === webglLoadToken) webglAddon = null;
    } finally {
      if (loadToken === webglLoadToken) {
        webglLoadPromise = null;
        const current = getContext();
        if (
          loadSkipped &&
          !webglAddon &&
          !current.disposed &&
          current.terminal &&
          current.terminalWebgl &&
          current.isForegroundRuntime
        ) {
          queueMicrotask(() => syncTerminalRenderer(current.generation));
        }
      }
    }
  }

  return {
    disposeOptionalAddons,
    schedulePostOpenTerminalAddons,
    syncTerminalRenderer,
  };
}
