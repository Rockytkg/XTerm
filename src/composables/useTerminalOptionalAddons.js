// WebGL context 丢失（GPU 重置、驱动崩溃）后的自动恢复退避。尝试耗尽后保持
// DOM 渲染器；返回前台时的常规 sync 也会重新尝试加载。
const WEBGL_CONTEXT_RECOVERY_DELAYS_MS = [2_000, 5_000, 15_000];

export function useTerminalOptionalAddons({ logger, getContext, loadAddon }) {
  let webglAddon = null;
  let webglLoadPromise = null;
  let webglLoadToken = 0;
  let webglRecoveryTimer = undefined;
  let webglRecoveryAttempt = 0;
  let webglUnavailableGeneration = null;
  let ligaturesAddon = null;
  let ligaturesLoadHandle = null;
  let ligaturesLoadPromise = null;

  function refreshTerminal(terminal) {
    if (!terminal) return;
    terminal.refresh(0, Math.max(0, terminal.rows - 1));
  }

  function disposeOptionalAddons() {
    cancelLigaturesLoad();
    cancelWebglRecovery();
    disposeWebglAddon();
    webglLoadToken += 1;
    webglLoadPromise = null;
    webglRecoveryAttempt = 0;
    webglUnavailableGeneration = null;
    ligaturesAddon?.dispose?.();
    ligaturesAddon = null;
    ligaturesLoadPromise = null;
  }

  function cancelWebglRecovery() {
    clearTimeout(webglRecoveryTimer);
    webglRecoveryTimer = undefined;
  }

  function disposeWebglAddon() {
    webglAddon?.dispose?.();
    webglAddon = null;
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
      void Promise.resolve(syncTerminalRenderer(current.generation))
        .then(() => {
          // 恢复失败（GPU 仍在重置）继续退避；成功时 sync 内已清零计数。
          if (!webglAddon) scheduleWebglRecovery();
        })
        .catch(() => {});
    }, WEBGL_CONTEXT_RECOVERY_DELAYS_MS[webglRecoveryAttempt]);
  }

  async function syncTerminalRenderer(generation) {
    const context = getContext();
    // 调用方可能不传 generation（如偏好开关 watcher）；缺省取当前实例的
    // generation，否则 `current.generation !== undefined` 恒真会让加载总是
    // 先被跳过、再靠重排队兜底。
    const resolvedGeneration = generation ?? context.generation;
    if (!context.terminal) return;
    if (!context.terminalWebgl) {
      cancelWebglRecovery();
      disposeWebglAddon();
      webglLoadToken += 1;
      webglLoadPromise = null;
      webglUnavailableGeneration = null;
      return;
    }
    // 后台 tab 不新建 GL 上下文（从未查看的 tab 因此不占 GPU 资源）；已加载的
    // 在后台也不 dispose——xterm 的 IntersectionObserver 已使不可见终端的渲染
    // 完全暂停，驻留上下文只占少量显存、零 CPU/GPU 开销，而 dispose 会让每次
    // 切回前台都重建上下文与字形图集（getImageData 回读 + 全视口 refresh）。
    // 浏览器对过多 WebGL 上下文的 LRU 驱逐由 onContextLoss 降级路径兜底。
    if (!context.isForegroundRuntime) return;
    if (webglAddon || webglLoadPromise) return webglLoadPromise;
    // 加载硬失败（通常是 WebGL2 不可用这类环境原因）后，同一 generation 内
    // 每次回前台重试只会刷日志；开关切换或终端重建时复位。
    if (webglUnavailableGeneration === resolvedGeneration) return;

    let loadSkipped = false;
    const loadToken = ++webglLoadToken;
    // 该 promise 会被并发调用方收养（`return webglLoadPromise`）：失败必须
    // 在这里吞掉（已记日志），否则收养方各自产生 unhandled rejection。
    webglLoadPromise = (async () => {
      try {
        const { WebglAddon } = await import("@xterm/addon-webgl");
        const current = getContext();
        if (
          loadToken !== webglLoadToken ||
          current.disposed ||
          current.generation !== resolvedGeneration ||
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
        webglUnavailableGeneration = null;
        // WebGL is loaded after Terminal.open(). Rebuild the complete viewport so
        // the new renderer does not inherit a stale render state.
        refreshTerminal(current.terminal);
      } catch (error) {
        logger.warn("WebGL renderer failed to load:", error);
        if (loadToken === webglLoadToken) {
          webglAddon = null;
          webglUnavailableGeneration = resolvedGeneration;
        }
      }
    })();

    await webglLoadPromise;
    if (loadToken !== webglLoadToken) return;
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

  return {
    disposeOptionalAddons,
    schedulePostOpenTerminalAddons,
    syncTerminalRenderer,
  };
}
