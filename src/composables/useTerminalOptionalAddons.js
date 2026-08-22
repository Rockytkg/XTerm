const WEBGL_INACTIVE_DISPOSE_DELAY_MS = 30_000;

export function useTerminalOptionalAddons({ logger, getContext, loadAddon }) {
  let webglAddon = null;
  let webglLoadPromise = null;
  let webglLoadToken = 0;
  let webglDisposeTimer = undefined;
  let ligaturesAddon = null;
  let ligaturesLoadHandle = null;
  let ligaturesLoadPromise = null;

  function disposeOptionalAddons() {
    cancelLigaturesLoad();
    cancelWebglDispose();
    disposeWebglAddon();
    webglLoadToken += 1;
    webglLoadPromise = null;
    ligaturesAddon?.dispose?.();
    ligaturesAddon = null;
    ligaturesLoadPromise = null;
  }

  function cancelWebglDispose() {
    clearTimeout(webglDisposeTimer);
    webglDisposeTimer = undefined;
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

  async function syncTerminalRenderer(generation) {
    const { terminal, isForegroundRuntime, terminalWebgl } = getContext();
    if (!terminal) return;
    if (!terminalWebgl) {
      cancelWebglDispose();
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
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        logger.warn("WebGL context lost, falling back to canvas renderer");
        if (webglAddon === addon) disposeWebglAddon();
      });
      current.terminal.loadAddon(addon);
      webglAddon = addon;
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
