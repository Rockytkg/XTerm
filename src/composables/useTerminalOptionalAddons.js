// WebGL context 丢失（GPU 重置、驱动崩溃）后的自动恢复退避。尝试耗尽后保持
// DOM 渲染器；返回前台时的常规 sync 也会重新尝试加载。
const WEBGL_CONTEXT_RECOVERY_DELAYS_MS = [2_000, 5_000, 15_000];
// 在滚动时间窗口内反复丢失上下文通常表示 GPU/驱动不稳定。用时间戳滑窗而不是
// 一个长期存活的“稳定 30s”定时器，避免后台终端仅为记账保留定时器。
const WEBGL_CONTEXT_LOSS_WINDOW_MS = 30_000;
const WEBGL_MAX_RAPID_LOSSES = 3;
// 软件光栅（SwiftShader / D3D11 WARP / llvmpipe）下 WebGL 的每帧全视口绘制
// 全部落在 CPU，稳定慢于 DOM 渲染器的按行 patch，识别后直接留在 DOM。
// `--disable-software-rasterizer` 只挡 SwiftShader，挡不住 ANGLE 选择 WARP。
const SOFTWARE_RENDERER_PATTERN =
  /swiftshader|llvmpipe|softpipe|software rasterizer|microsoft basic render driver/i;

let detectedRenderer = null;

function detectWebglRenderer() {
  if (detectedRenderer) return detectedRenderer;
  if (typeof document === "undefined") return { renderer: "", software: false };
  const canvas = document.createElement("canvas");
  let gl = null;
  try {
    gl = canvas.getContext("webgl2", { antialias: false, depth: false });
    if (!gl) return { renderer: "", software: false };
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    const renderer = String(
      (info && gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) || gl.getParameter(gl.RENDERER) || "",
    );
    detectedRenderer = { renderer, software: SOFTWARE_RENDERER_PATTERN.test(renderer) };
  } catch {
    detectedRenderer = { renderer: "", software: false };
  } finally {
    gl?.getExtension("WEBGL_lose_context")?.loseContext?.();
  }
  return detectedRenderer;
}

export function useTerminalOptionalAddons({
  logger,
  getContext,
  loadAddon,
  detectRenderer = detectWebglRenderer,
}) {
  let webglAddon = null;
  let webglLoadPromise = null;
  let webglLoadToken = 0;
  let webglRecoveryTimer = undefined;
  let webglRecoveryAttempt = 0;
  let webglUnavailableGeneration = null;
  let webglLossTimes = [];
  let ligaturesAddon = null;
  let ligaturesLoadHandle = null;
  let ligaturesLoadPromise = null;

  function cancelWebglRecovery() {
    clearTimeout(webglRecoveryTimer);
    webglRecoveryTimer = undefined;
  }

  function disposeWebglAddon() {
    webglAddon?.dispose?.();
    webglAddon = null;
  }

  function resetWebglRenderer() {
    cancelWebglRecovery();
    disposeWebglAddon();
    webglLoadToken += 1;
    webglLoadPromise = null;
    webglRecoveryAttempt = 0;
    webglUnavailableGeneration = null;
    webglLossTimes = [];
  }

  function disposeOptionalAddons() {
    cancelLigaturesLoad();
    resetWebglRenderer();
    ligaturesAddon?.dispose?.();
    ligaturesAddon = null;
    ligaturesLoadPromise = null;
  }

  async function reloadWebglRenderer(generation) {
    const context = getContext();
    resetWebglRenderer();
    if (context.terminalWebgl && context.isForegroundRuntime) {
      await syncTerminalRenderer(generation ?? context.generation);
    }
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
      const { TauriLigaturesAddon } =
        await import("../utils/terminal/addons/ligatures/TauriLigaturesAddon");
      const current = getContext();
      if (current.disposed || current.generation !== generation || !current.terminal) return;
      loadAddon(
        "ligatures",
        () => new TauriLigaturesAddon(),
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
      void Promise.resolve(syncTerminalRenderer(current.generation, { recovery: true }))
        .then(() => {
          // 恢复失败（GPU 仍在重置）继续退避；成功时 sync 内已清零计数。
          if (!webglAddon) scheduleWebglRecovery();
        })
        .catch(() => {});
    }, WEBGL_CONTEXT_RECOVERY_DELAYS_MS[webglRecoveryAttempt]);
  }

  async function syncTerminalRenderer(generation, { recovery = false } = {}) {
    const context = getContext();
    // 调用方可能不传 generation（如偏好开关 watcher）；缺省取当前实例的
    // generation，否则 `current.generation !== undefined` 恒真会让加载总是
    // 先被跳过、再靠重排队兜底。
    const resolvedGeneration = generation ?? context.generation;
    if (!context.terminal) return;
    if (!context.terminalWebgl) {
      resetWebglRenderer();
      return;
    }
    // 后台 tab 不新建 GL 上下文（从未查看的 tab 因此不占 GPU 资源）；已加载的
    // 在后台也不 dispose——xterm 的 IntersectionObserver 已使不可见终端的渲染
    // 完全暂停，驻留上下文只占少量显存、零 CPU/GPU 开销，而 dispose 会让每次
    // 切回前台都重建上下文与字形图集（getImageData 回读 + 全视口 refresh）。
    // 浏览器对过多 WebGL 上下文的 LRU 驱逐由 onContextLoss 降级路径兜底。
    if (!context.isForegroundRuntime) return;
    if (webglAddon || webglLoadPromise) return webglLoadPromise;
    // 初始加载的硬失败（WebGL2 不可用）在同一 generation 内不再重试；但上下文
    // 丢失后的恢复是瞬时的（GPU 进程重置期间 getContext 会短暂返回 null），
    // 不能落闩锁，否则第 2/3 次退避直接空转，终端会永久停在 DOM。
    if (!recovery && webglUnavailableGeneration === resolvedGeneration) return;

    let loadSkipped = false;
    const loadToken = ++webglLoadToken;
    // 该 promise 会被并发调用方收养（`return webglLoadPromise`）：失败必须
    // 在这里吞掉（已记日志），否则收养方各自产生 unhandled rejection。
    webglLoadPromise = (async () => {
      try {
        const detected = await detectRenderer();
        if (detected.software) {
          if (loadToken === webglLoadToken) {
            webglUnavailableGeneration = resolvedGeneration;
          }
          logger.warn(
            `WebGL renderer is software rasterized (${detected.renderer || "unknown"}); keeping the DOM renderer`,
          );
          return;
        }
        if (detected.renderer) logger.info("WebGL renderer:", detected.renderer);
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
          if (webglAddon !== addon) return;
          const now = Date.now();
          webglLossTimes = webglLossTimes.filter(
            (timestamp) => now - timestamp < WEBGL_CONTEXT_LOSS_WINDOW_MS,
          );
          webglLossTimes.push(now);
          disposeWebglAddon();
          if (webglLossTimes.length >= WEBGL_MAX_RAPID_LOSSES) {
            // GPU 连续崩溃：继续在 DOM 与 WebGL 之间来回切换只会让画面反复
            // 闪烁，停在 DOM 直到用户切换开关或终端重建。
            webglUnavailableGeneration = current.generation;
            logger.warn(
              `WebGL context lost ${webglLossTimes.length} times within ${WEBGL_CONTEXT_LOSS_WINDOW_MS / 1000}s; staying on the DOM renderer`,
            );
            return;
          }
          logger.warn("WebGL context lost, falling back to the DOM renderer");
          scheduleWebglRecovery();
        });
        current.terminal.loadAddon(addon);
        webglAddon = addon;
        webglRecoveryAttempt = 0;
        webglUnavailableGeneration = null;
      } catch (error) {
        logger.warn(
          recovery ? "WebGL renderer recovery attempt failed:" : "WebGL renderer failed to load:",
          error,
        );
        if (loadToken === webglLoadToken) {
          webglAddon = null;
          if (!recovery) webglUnavailableGeneration = resolvedGeneration;
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
    reloadWebglRenderer,
    schedulePostOpenTerminalAddons,
    syncTerminalRenderer,
  };
}
