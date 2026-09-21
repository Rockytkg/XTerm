<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ClipboardPaste, Keyboard, Maximize, Scaling } from "@lucide/vue";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useToasts } from "../composables/useToasts";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { createLogger } from "../utils/logger";
import { scancodeForKeyEvent } from "../utils/rdpScancodeMap";
import { createWheelAccumulator } from "../utils/wheelAccumulator";
import {
  RDP_SERVER_MESSAGE,
  decodeServerMessage,
  encodeClipboardText,
  encodeKey,
  encodePointerButton,
  encodePointerMove,
  encodeResizeRequest,
  encodeWheel,
} from "../utils/rdpCodec";

const SCALE_MODES = ["fit", "none", "clip"];
const AUTH_RETRY_ERROR_CODES = new Set(["rdp_auth_required", "rdp_auth_failed"]);
const RESIZE_DEBOUNCE_MS = 300;

const props = defineProps({
  activeConnection: { type: Object, default: null },
  connectionState: {
    type: Object,
    default: () => ({ status: "idle", phase: null, error: null }),
  },
  frontendSessionId: { type: String, default: "" },
  rdpBridge: { type: Object, default: null },
});

const emit = defineEmits(["retryConnection"]);

const { t, te } = useI18n();
const { showToast } = useToasts();
const { sessionRegistry } = useWorkspaceStore();
const logger = createLogger("frontend.terminal.rdp");

const desktopViewport = ref(null);
const desktopCanvas = ref(null);
const bridgeConnected = ref(false);
// 桥侧/WS 侧的错误与断开原因，与后端 connectionState 互补。
const bridgeError = ref("");
const disconnectReason = ref("");
const desktopSize = ref({ width: 0, height: 0 });
const containerSize = ref({ width: 0, height: 0 });
const passwordInput = ref("");

let ws = null;
let canvasContext = null;
let resizeDebounceTimer = 0;
let resizeObserver = null;
// 已按下未释放的按键，blur/断线时统一补 keyup，避免远端按键卡死。
const pressedKeys = new Map();

// 攒够一个滚轮刻度才发一次 Wheel 消息（单位"格"，正=向上）；浏览器 deltaY 向下
// 为正，桥协议向上为正，发送时取反。
const wheelAccumulator = createWheelAccumulator({
  send: (direction) => sendFrame(encodeWheel(-direction)),
  getPageHeightPx: () => desktopViewport.value?.clientHeight || 600,
});

function sendFrame(buffer) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return false;
  ws.send(buffer);
  return true;
}

function releaseAllKeys() {
  for (const entry of pressedKeys.values()) {
    sendFrame(encodeKey(entry.scancode, false, entry.extended));
  }
  pressedKeys.clear();
}

function handleDesktopWheel(event) {
  if (!bridgeConnected.value) return;
  // 横向滚动不转发（RDP 桥协议只有垂直 Wheel）。
  if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
  event.preventDefault();
  wheelAccumulator.feed(event);
}

const connectionStatus = computed(() => props.connectionState?.status || "idle");
const authRetryRequested = computed(() =>
  AUTH_RETRY_ERROR_CODES.has(props.connectionState?.error?.code || ""),
);

const endpointLabel = computed(() => {
  const host = props.activeConnection?.host || props.activeConnection?.name || "-";
  const port = props.activeConnection?.port ? `:${props.activeConnection.port}` : "";
  return `${host}${port}`;
});

const failureLabel = computed(() => {
  const error = props.connectionState?.error;
  if (!error) return "";
  const code = error.code || "unknown";
  const key = `connectionErrors.${code}`;
  return te(key) ? t(key, error.args || {}) : error.detail || t("connectionErrors.unknown");
});

const failureDetail = computed(() => {
  const detail = props.connectionState?.error?.detail || props.connectionState?.statusDetail;
  return typeof detail === "string" ? detail.trim() : "";
});

const scaleMode = computed(() => props.activeConnection?.rdpScaleMode || "fit");

// 与接收路径（CLIPBOARD_TEXT）的门控口径一致：默认开，仅显式 false 关闭。
const clipboardSyncEnabled = computed(() => props.activeConnection?.rdpClipboardSync !== false);

// 把协商后的实际分辨率回写 registry（保留 url 等字段，避免触发重连 watch），
// 侧边栏"分辨率"展示才能跟随 Display Control 动态分辨率调整。
function syncBridgeSize(width, height) {
  const bridge = props.rdpBridge;
  if (!props.frontendSessionId || !bridge) return;
  if (bridge.width === width && bridge.height === height) return;
  sessionRegistry.setSessionRdpBridge(props.frontendSessionId, { ...bridge, width, height });
}

// 单一状态浮层：优先级 桥错误 > 连接中 > 失败 > 已断开。
const overlayKind = computed(() => {
  if (bridgeError.value) return "error";
  if (connectionStatus.value === "connecting") return "connecting";
  if (connectionStatus.value === "failed") return "failed";
  if (connectionStatus.value === "closed" && !bridgeConnected.value) return "closed";
  return null;
});

const overlayTitle = computed(() => {
  switch (overlayKind.value) {
    case "error":
      return t("rdpDesktop.connectionError");
    case "connecting":
      return t("terminal.connectionConnectingRdp");
    case "failed":
      return failureLabel.value || t("connectionErrors.unknown");
    case "closed":
      return t("rdpDesktop.sessionClosed");
    default:
      return "";
  }
});

const overlayDescription = computed(() => {
  switch (overlayKind.value) {
    case "error":
      return bridgeError.value;
    case "connecting":
      return endpointLabel.value;
    case "failed":
      return failureDetail.value;
    case "closed":
      return disconnectReason.value;
    default:
      return "";
  }
});

const overlayNeedsPassword = computed(
  () => overlayKind.value === "failed" && authRetryRequested.value,
);

const overlaySubmitDisabled = computed(
  () => overlayNeedsPassword.value && !passwordInput.value,
);

function handleOverlaySubmit() {
  if (overlayKind.value === "failed" && authRetryRequested.value) {
    retryConnection({ rdpPassword: passwordInput.value });
    passwordInput.value = "";
    return;
  }
  retryConnection();
}

function applyDesktopSize(width, height) {
  desktopSize.value = { width, height };
  const canvas = desktopCanvas.value;
  if (!canvas) return;
  if (canvas.width !== width) canvas.width = width;
  if (canvas.height !== height) canvas.height = height;
  // canvas 尺寸赋值不会使已取得的 context 失效，只取一次；帧全不透明，省合成开销。
  if (!canvasContext) canvasContext = canvas.getContext("2d", { alpha: false });
}

function applyFrame(rects) {
  const context = canvasContext;
  if (!context) return;
  for (const rect of rects) {
    if (!rect.width || !rect.height) continue;
    context.putImageData(new ImageData(rect.pixels, rect.width, rect.height), rect.x, rect.y);
  }
}

function handleServerMessage(buffer) {
  let message;
  try {
    message = decodeServerMessage(buffer);
  } catch (error) {
    logger.warn("bridge.decode.failed", error);
    return;
  }
  if (!message) return;
  switch (message.type) {
    case RDP_SERVER_MESSAGE.HELLO:
    case RDP_SERVER_MESSAGE.RESIZE:
      applyDesktopSize(message.width, message.height);
      syncBridgeSize(message.width, message.height);
      break;
    case RDP_SERVER_MESSAGE.FRAME:
      applyFrame(message.rects);
      break;
    case RDP_SERVER_MESSAGE.CLIPBOARD_TEXT: {
      if (props.activeConnection?.rdpClipboardSync === false) break;
      if (!message.text) break;
      writeText(message.text).catch((error) => logger.warn("clipboard.write.failed", error));
      break;
    }
    case RDP_SERVER_MESSAGE.DISCONNECT:
      disconnectReason.value = message.reason || "";
      destroyBridge();
      break;
    default:
      break;
  }
}

function destroyBridge() {
  const socket = ws;
  ws = null;
  bridgeConnected.value = false;
  wheelAccumulator.reset();
  if (resizeDebounceTimer) {
    window.clearTimeout(resizeDebounceTimer);
    resizeDebounceTimer = 0;
  }
  if (socket) {
    // 销毁中不再关心 close/error 回调。
    socket.onclose = null;
    socket.onerror = null;
    socket.onmessage = null;
    try {
      socket.close();
    } catch (error) {
      logger.warn("bridge.close.failed", error);
    }
  }
}

function connectBridge() {
  const url = props.rdpBridge?.url || "";
  if (!url || ws) return;
  let socket;
  try {
    socket = new WebSocket(url, "binary");
  } catch (error) {
    logger.error("bridge.create.failed", error);
    bridgeError.value = String(error);
    return;
  }
  ws = socket;
  socket.binaryType = "arraybuffer";
  disconnectReason.value = "";
  if (props.rdpBridge?.width && props.rdpBridge?.height) {
    applyDesktopSize(props.rdpBridge.width, props.rdpBridge.height);
  }

  socket.onopen = () => {
    bridgeConnected.value = true;
  };
  socket.onmessage = (event) => {
    if (typeof event.data === "string") return;
    handleServerMessage(event.data);
  };
  socket.onerror = (event) => {
    logger.warn("bridge.socket.error", { readyState: socket.readyState, type: event?.type });
  };
  socket.onclose = (event) => {
    if (ws !== socket) return;
    const wasConnected = bridgeConnected.value;
    ws = null;
    bridgeConnected.value = false;
    pressedKeys.clear();
    // 后端桥在 RDP 断开时会同步把 connectionState 置为 closed；这里只补充
    // WebSocket 自身异常断开（后端仍认为 connected）的情况。
    if (!event.wasClean && wasConnected && connectionStatus.value === "connected") {
      bridgeError.value = t("rdpDesktop.disconnectedUnexpectedly");
    }
  };
}

function retryConnection(options = {}) {
  bridgeError.value = "";
  emit("retryConnection", options);
}

async function sendLocalClipboard() {
  if (!bridgeConnected.value) return;
  try {
    const text = await readText();
    if (text) sendFrame(encodeClipboardText(text));
  } catch (error) {
    logger.warn("clipboard.read.failed", error);
    showToast({ type: "error", title: t("rdpDesktop.clipboardSendFailed") });
  }
}

function pressKey(code, pressed) {
  const entry = scancodeForKeyEvent({ code });
  if (!entry) return;
  sendFrame(encodeKey(entry.scancode, pressed, entry.extended));
}

// 远端登录界面需要 Ctrl+Alt+Del 解锁；浏览器拿不到这个组合，由工具栏合成。
function sendCtrlAltDel() {
  if (!bridgeConnected.value) return;
  for (const code of ["ControlLeft", "AltLeft", "Delete"]) pressKey(code, true);
  for (const code of ["Delete", "AltLeft", "ControlLeft"]) pressKey(code, false);
}

// 工具条循环切换的本地缩放覆盖；profile 的 scaleMode 变化时清空回退。
const scaleModeLocal = ref("");

function cycleScaleMode() {
  const current = scaleModeLocal.value || scaleMode.value;
  const next = SCALE_MODES[(SCALE_MODES.indexOf(current) + 1) % SCALE_MODES.length];
  scaleModeLocal.value = next;
}

const effectiveScaleMode = computed(() => scaleModeLocal.value || scaleMode.value);

const scaleModeLabel = computed(() =>
  t(`connectionDialog.rdp.scaleModes.${effectiveScaleMode.value}`),
);

// fit 模式按容器等比缩放画布（CSS 尺寸缩放，像素仍由桥帧填充）。
const fitScale = computed(() => {
  const { width, height } = desktopSize.value;
  const { width: containerWidth, height: containerHeight } = containerSize.value;
  if (!width || !height || !containerWidth || !containerHeight) return 1;
  return Math.min(containerWidth / width, containerHeight / height);
});

const canvasStyle = computed(() => {
  if (effectiveScaleMode.value !== "fit") return {};
  const { width, height } = desktopSize.value;
  if (!width || !height) return {};
  return {
    width: `${Math.max(1, Math.round(width * fitScale.value))}px`,
    height: `${Math.max(1, Math.round(height * fitScale.value))}px`,
  };
});

function toggleFullscreen() {
  const viewport = desktopViewport.value;
  if (!viewport) return;
  if (document.fullscreenElement) {
    void document.exitFullscreen?.();
  } else {
    void viewport.requestFullscreen?.();
  }
}

// 画布显示尺寸（fit 下被 CSS 缩放）换算回桌面像素坐标。
function desktopPointFromEvent(event) {
  const canvas = desktopCanvas.value;
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return null;
  const x = Math.round(((event.clientX - rect.left) * canvas.width) / rect.width);
  const y = Math.round(((event.clientY - rect.top) * canvas.height) / rect.height);
  return {
    x: Math.min(Math.max(x, 0), canvas.width - 1),
    y: Math.min(Math.max(y, 0), canvas.height - 1),
  };
}

function handlePointerMove(event) {
  if (!bridgeConnected.value) return;
  const point = desktopPointFromEvent(event);
  if (!point) return;
  sendFrame(encodePointerMove(point.x, point.y));
}

const RDP_BUTTON_BY_POINTER = { 0: 0, 1: 2, 2: 1 };

function handlePointerButton(event, pressed) {
  if (!bridgeConnected.value) return;
  const button = RDP_BUTTON_BY_POINTER[event.button];
  if (button === undefined) return;
  event.preventDefault();
  if (pressed) {
    // 按下后若指针移出画布再松开，没有捕获的话 pointerup 会派发给别的元素，
    // 远端永远收不到 button release 导致按键卡死；捕获在 pointerup 后自动释放。
    try {
      event.currentTarget?.setPointerCapture(event.pointerId);
    } catch {
      // 指针已失效（如触摸点被取消）时忽略，不影响本次按下。
    }
  }
  desktopCanvas.value?.focus();
  sendFrame(encodePointerButton(button, pressed));
}

function handleKeyEvent(event, pressed) {
  if (!bridgeConnected.value) return;
  const entry = scancodeForKeyEvent(event);
  if (!entry) return;
  // 已映射的键一律吞掉浏览器默认行为（Ctrl+W 关窗、Ctrl+Tab 切页等），
  // 让焦点内的组合键全部直达远端。未映射的键放行。
  event.preventDefault();
  const key = `${entry.extended}:${entry.scancode}`;
  if (pressed) {
    pressedKeys.set(key, entry);
  } else {
    pressedKeys.delete(key);
  }
  sendFrame(encodeKey(entry.scancode, pressed, entry.extended));
}

function handleCanvasBlur() {
  releaseAllKeys();
}

function scheduleResizeRequest() {
  if (!bridgeConnected.value) return;
  if (props.activeConnection?.rdpResizeSession !== true) return;
  const viewport = desktopViewport.value;
  if (!viewport) return;
  if (resizeDebounceTimer) window.clearTimeout(resizeDebounceTimer);
  resizeDebounceTimer = window.setTimeout(() => {
    resizeDebounceTimer = 0;
    // 偶数对齐：RDP 显示通道要求宽高为 2 的倍数。
    const width = Math.max(2, viewport.clientWidth & ~1);
    const height = Math.max(2, viewport.clientHeight & ~1);
    const { width: currentWidth, height: currentHeight } = desktopSize.value;
    if (width === currentWidth && height === currentHeight) return;
    sendFrame(encodeResizeRequest(width, height));
  }, RESIZE_DEBOUNCE_MS);
}

watch(
  () => [connectionStatus.value, props.rdpBridge?.url],
  ([status, url]) => {
    if (status === "connected" && url) {
      connectBridge();
      return;
    }
    destroyBridge();
    if (status === "connecting") {
      bridgeError.value = "";
      disconnectReason.value = "";
      passwordInput.value = "";
    }
  },
  { immediate: true },
);

watch(scaleMode, () => {
  scaleModeLocal.value = "";
});

onMounted(() => {
  // setup 阶段的 immediate watch 可能已连桥并写入 desktopSize，彼时 canvas 尚未
  // 挂载（早退在 300×150 默认尺寸）；挂载后按已知尺寸补一次应用。
  if (desktopSize.value.width && desktopSize.value.height) {
    applyDesktopSize(desktopSize.value.width, desktopSize.value.height);
  }
  // 非 passive：需要 preventDefault 阻止页面滚动。
  desktopViewport.value?.addEventListener("wheel", handleDesktopWheel, { passive: false });
  resizeObserver = new ResizeObserver(() => {
    const viewport = desktopViewport.value;
    if (!viewport) return;
    containerSize.value = { width: viewport.clientWidth, height: viewport.clientHeight };
    scheduleResizeRequest();
  });
  if (desktopViewport.value) resizeObserver.observe(desktopViewport.value);
});

onBeforeUnmount(() => {
  desktopViewport.value?.removeEventListener("wheel", handleDesktopWheel);
  resizeObserver?.disconnect();
  resizeObserver = null;
  destroyBridge();
});
</script>

<template>
  <article class="rdp-workspace flex flex-1 flex-col overflow-hidden">
    <div
      v-show="bridgeConnected"
      class="rdp-toolbar"
    >
      <span
        class="rdp-toolbar-title"
        :title="endpointLabel"
      >{{ endpointLabel }}</span>
      <div class="rdp-toolbar-actions">
        <button
          type="button"
          class="ui-icon-button"
          :title="t('rdpDesktop.ctrlAltDel')"
          :aria-label="t('rdpDesktop.ctrlAltDel')"
          @click="sendCtrlAltDel"
        >
          <Keyboard
            :size="15"
            stroke-width="1.8"
          />
        </button>
        <button
          v-if="clipboardSyncEnabled"
          type="button"
          class="ui-icon-button"
          :title="t('rdpDesktop.sendClipboard')"
          :aria-label="t('rdpDesktop.sendClipboard')"
          @click="void sendLocalClipboard"
        >
          <ClipboardPaste
            :size="15"
            stroke-width="1.8"
          />
        </button>
        <button
          type="button"
          class="ui-icon-button"
          :title="t('rdpDesktop.scaleMode', { mode: scaleModeLabel })"
          :aria-label="t('rdpDesktop.scaleMode', { mode: scaleModeLabel })"
          @click="cycleScaleMode"
        >
          <Scaling
            :size="15"
            stroke-width="1.8"
          />
        </button>
        <button
          type="button"
          class="ui-icon-button"
          :title="t('rdpDesktop.fullscreen')"
          :aria-label="t('rdpDesktop.fullscreen')"
          @click="toggleFullscreen"
        >
          <Maximize
            :size="15"
            stroke-width="1.8"
          />
        </button>
      </div>
    </div>

    <section class="rdp-surface min-h-0 flex-1 overflow-hidden">
      <div
        ref="desktopViewport"
        class="rdp-viewport"
        :class="`rdp-viewport--${effectiveScaleMode}`"
      >
        <!-- tabindex 使画布可聚焦以接收键盘输入；IME 组合输入暂不支持
             （只转发物理扫描码），中文输入需要在远端切输入法。
             v-show：连接建立前/断开后隐藏画布，避免默认 300×150 的黑块
             或断开前的最后一帧透过半透明浮层露出。 -->
        <canvas
          v-show="bridgeConnected"
          ref="desktopCanvas"
          class="rdp-canvas"
          :style="canvasStyle"
          tabindex="0"
          @contextmenu.prevent
          @keydown="handleKeyEvent($event, true)"
          @keyup="handleKeyEvent($event, false)"
          @blur="handleCanvasBlur"
          @pointermove="handlePointerMove"
          @pointerdown="handlePointerButton($event, true)"
          @pointerup="handlePointerButton($event, false)"
        />
      </div>

      <div
        v-if="overlayKind"
        class="rdp-overlay"
      >
        <form
          class="rdp-overlay-card"
          @submit.prevent="handleOverlaySubmit"
        >
          <span
            v-if="overlayKind === 'connecting'"
            class="rdp-spinner"
          />
          <h3 class="rdp-overlay-title">
            {{ overlayTitle }}
          </h3>
          <p
            v-if="overlayDescription"
            class="rdp-overlay-desc"
          >
            {{ overlayDescription }}
          </p>
          <input
            v-if="overlayNeedsPassword"
            v-model="passwordInput"
            type="password"
            class="ui-input ui-fill-inline"
            :placeholder="t('connectionDialog.fields.password')"
            autocomplete="off"
            autofocus
          >
          <button
            v-if="overlayKind !== 'connecting'"
            type="submit"
            class="ui-button-primary"
            :disabled="overlaySubmitDisabled"
          >
            {{ t("actions.reconnect") }}
          </button>
        </form>
      </div>
    </section>
  </article>
</template>

<style lang="scss" scoped>
.rdp-workspace {
  position: relative;
  background: var(--bg-primary, #111);
}

.rdp-surface {
  position: relative;
}

.rdp-viewport {
  position: absolute;
  inset: 0;

  &--fit {
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  &--none {
    overflow: auto;
  }

  &--clip {
    overflow: hidden;
  }
}

.rdp-canvas {
  display: block;
  outline: none;
  background: #000;
}

.rdp-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 10px;
  border-bottom: 1px solid var(--border-light);
  background: var(--bg-secondary, transparent);
  flex-shrink: 0;
}

.rdp-toolbar-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}

.rdp-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.rdp-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in oklch, var(--bg-primary, #111) 72%, transparent);
  z-index: 10;
}

.rdp-overlay-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(340px, 80%);
  padding: 20px;
  border: 1px solid var(--border-light);
  border-radius: 10px;
  background: var(--bg-secondary, #1c1c1c);
  box-shadow: 0 8px 28px rgb(0 0 0 / 35%);
}

.rdp-overlay-title {
  margin: 0;
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--text-primary);
  text-align: center;
}

.rdp-overlay-desc {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
  text-align: center;
  word-break: break-all;
}

.rdp-spinner {
  align-self: center;
  width: 22px;
  height: 22px;
  border: 2px solid var(--border-light);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: rdp-spin 0.8s linear infinite;
}

@keyframes rdp-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
