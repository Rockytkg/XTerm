<script setup>
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { ClipboardPaste, Keyboard, Maximize, Scaling } from "@lucide/vue";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useToasts } from "../composables/useToasts";
import { createLogger } from "../utils/logger";

const SCALE_MODES = ["fit", "none", "clip"];
const AUTH_RETRY_ERROR_CODES = new Set(["vnc_auth_required", "vnc_auth_failed"]);

const props = defineProps({
  activeConnection: { type: Object, default: null },
  connectionState: {
    type: Object,
    default: () => ({ status: "idle", phase: null, error: null }),
  },
  vncBridge: { type: Object, default: null },
});

const emit = defineEmits(["retryConnection"]);

const { t, te } = useI18n();
const { showToast } = useToasts();
const logger = createLogger("frontend.terminal.vnc");

const desktopMount = ref(null);
const desktopName = ref("");
const rfbConnected = ref(false);
// 密码等凭证由用户在弹层里输入，绝不从后端凭证库读取（passthrough 模式下
// noVNC 自己应答服务器认证挑战）。passwordInput 同时服务 passthrough 凭证
// 弹层与认证失败后的密码重试。
const credentialsPrompt = ref(null);
const credentialUsername = ref("");
const passwordInput = ref("");
// RFB 实例侧的错误（securityfailure / 非干净断开），与后端 connectionState 互补。
const rfbError = ref("");

let rfb = null;
let rfbLoadPromise = null;

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

const scaleMode = computed(() => props.activeConnection?.vncScaleMode || "fit");

// 单一状态浮层：优先级 凭证请求 > RFB 错误 > 连接中 > 失败 > 已断开。
const overlayKind = computed(() => {
  if (credentialsPrompt.value) return "credentials";
  if (rfbError.value) return "error";
  if (connectionStatus.value === "connecting") return "connecting";
  if (connectionStatus.value === "failed") return "failed";
  if (connectionStatus.value === "closed" && !rfbConnected.value) return "closed";
  return null;
});

const overlayTitle = computed(() => {
  switch (overlayKind.value) {
    case "credentials":
      return t("vncDesktop.credentialsTitle");
    case "error":
      return t("vncDesktop.connectionError");
    case "connecting":
      return t("terminal.connectionConnectingVnc");
    case "failed":
      return failureLabel.value || t("connectionErrors.unknown");
    case "closed":
      return t("vncDesktop.sessionClosed");
    default:
      return "";
  }
});

const overlayDescription = computed(() => {
  switch (overlayKind.value) {
    case "credentials":
      return t("vncDesktop.credentialsDescription");
    case "error":
      return rfbError.value;
    case "connecting":
      return endpointLabel.value;
    case "failed":
      return failureDetail.value;
    default:
      return "";
  }
});

const overlayNeedsPassword = computed(
  () =>
    overlayKind.value === "credentials" ||
    (overlayKind.value === "failed" && authRetryRequested.value),
);

const overlaySubmitDisabled = computed(
  () => overlayNeedsPassword.value && !passwordInput.value,
);

function handleOverlaySubmit() {
  if (overlayKind.value === "credentials") {
    submitCredentials();
    return;
  }
  if (overlayKind.value === "failed" && authRetryRequested.value) {
    retryConnection({ vncPassword: passwordInput.value });
    passwordInput.value = "";
    return;
  }
  retryConnection();
}

async function loadRfbClass() {
  if (!rfbLoadPromise) {
    rfbLoadPromise = import("@novnc/novnc").then((module) => module.default);
  }
  return rfbLoadPromise;
}

function applyScaleMode(instance, mode) {
  instance.scaleViewport = mode === "fit";
  instance.clipViewport = mode === "clip";
}

function applyLiveOptions(instance) {
  const connection = props.activeConnection || {};
  instance.viewOnly = connection.vncViewOnly === true;
  instance.qualityLevel = Number(connection.vncQuality ?? 6);
  instance.compressionLevel = Number(connection.vncCompression ?? 2);
  instance.resizeSession = connection.vncResizeSession === true;
  applyScaleMode(instance, scaleMode.value);
}

function clearDesktopMount() {
  // RFB 只在构造时向 target 追加 screen/canvas，disconnect 不会移除；
  // 不清理会在重连时叠加多个画布。
  const mount = desktopMount.value;
  if (mount) mount.replaceChildren();
}

function destroyRfb() {
  const instance = rfb;
  rfb = null;
  rfbConnected.value = false;
  if (instance) {
    try {
      instance.disconnect();
    } catch (error) {
      logger.warn("rfb.disconnect.failed", error);
    }
  }
  clearDesktopMount();
}

async function mountRfb() {
  const url = props.vncBridge?.url || "";
  if (!url || !desktopMount.value || rfb) return;
  let RFB;
  try {
    RFB = await loadRfbClass();
  } catch (error) {
    logger.error("rfb.module_load.failed", error);
    rfbError.value = String(error);
    return;
  }
  // 等待模块期间状态可能已经变化（断线/重连），重新校验再建实例。
  if (rfb || connectionStatus.value !== "connected" || props.vncBridge?.url !== url) return;
  if (!desktopMount.value) return;
  clearDesktopMount();

  let instance;
  try {
    instance = new RFB(desktopMount.value, url, {
      credentials: {},
      shared: props.activeConnection?.vncShared !== false,
    });
  } catch (error) {
    logger.error("rfb.create.failed", error);
    rfbError.value = String(error);
    return;
  }
  rfb = instance;
  desktopName.value = props.vncBridge?.desktopName || "";
  rfbError.value = "";
  applyLiveOptions(instance);

  instance.addEventListener("connect", () => {
    rfbConnected.value = true;
  });
  instance.addEventListener("desktopname", (event) => {
    desktopName.value = event.detail?.name || "";
  });
  instance.addEventListener("credentialsrequired", (event) => {
    credentialUsername.value = "";
    passwordInput.value = "";
    credentialsPrompt.value = { types: event.detail?.types || ["password"] };
  });
  instance.addEventListener("securityfailure", (event) => {
    rfbError.value = event.detail?.reason || t("connectionErrors.vnc_auth_failed");
  });
  instance.addEventListener("clipboard", (event) => {
    if (props.activeConnection?.vncClipboardSync === false) return;
    const text = event.detail?.text;
    if (!text) return;
    writeText(text).catch((error) => logger.warn("clipboard.write.failed", error));
  });
  instance.addEventListener("disconnect", (event) => {
    const wasConnected = rfbConnected.value;
    rfb = null;
    rfbConnected.value = false;
    credentialsPrompt.value = null;
    clearDesktopMount();
    // 后端桥在 TCP 断开时会同步把 connectionState 置为 closed；这里只补充
    // WebSocket 自身异常断开（后端仍认为 connected）的情况。
    if (!event.detail?.clean && wasConnected && connectionStatus.value === "connected") {
      rfbError.value = t("vncDesktop.disconnectedUnexpectedly");
    }
  });
}

function submitCredentials() {
  if (!rfb || !credentialsPrompt.value) return;
  const types = credentialsPrompt.value.types;
  const credentials = {};
  if (types.includes("username")) credentials.username = credentialUsername.value;
  if (types.includes("password")) credentials.password = passwordInput.value;
  credentialsPrompt.value = null;
  passwordInput.value = "";
  try {
    rfb.sendCredentials(credentials);
  } catch (error) {
    logger.warn("rfb.send_credentials.failed", error);
    rfbError.value = String(error);
  }
}

function retryConnection(options = {}) {
  rfbError.value = "";
  emit("retryConnection", options);
}

async function sendLocalClipboard() {
  if (!rfb || !rfbConnected.value) return;
  try {
    const text = await readText();
    if (text) rfb.clipboardPasteFrom(text);
  } catch (error) {
    logger.warn("clipboard.read.failed", error);
    showToast({ type: "error", title: t("vncDesktop.clipboardSendFailed") });
  }
}

function sendCtrlAltDel() {
  if (!rfb || !rfbConnected.value) return;
  rfb.sendCtrlAltDel();
}

// 工具条循环切换的本地缩放覆盖；profile 的 scaleMode 变化时清空回退。
const scaleModeLocal = ref("");

function cycleScaleMode() {
  if (!rfb) return;
  const current =
    scaleModeLocal.value || (rfb.scaleViewport ? "fit" : rfb.clipViewport ? "clip" : "none");
  const next = SCALE_MODES[(SCALE_MODES.indexOf(current) + 1) % SCALE_MODES.length];
  scaleModeLocal.value = next;
  applyScaleMode(rfb, next);
}

const scaleModeLabel = computed(() =>
  t(`connectionDialog.vnc.scaleModes.${scaleModeLocal.value || scaleMode.value}`),
);

function toggleFullscreen() {
  const mount = desktopMount.value;
  if (!mount) return;
  if (document.fullscreenElement) {
    void document.exitFullscreen?.();
  } else {
    void mount.requestFullscreen?.();
  }
}

watch(
  () => [connectionStatus.value, props.vncBridge?.url],
  ([status, url]) => {
    if (status === "connected" && url) {
      void mountRfb();
      return;
    }
    destroyRfb();
    if (status === "connecting") {
      rfbError.value = "";
      credentialsPrompt.value = null;
      passwordInput.value = "";
    }
  },
  { immediate: true },
);

watch(
  () => [
    props.activeConnection?.vncViewOnly,
    props.activeConnection?.vncQuality,
    props.activeConnection?.vncCompression,
    props.activeConnection?.vncResizeSession,
  ],
  () => {
    if (rfb) applyLiveOptions(rfb);
  },
);

watch(scaleMode, (mode) => {
  scaleModeLocal.value = "";
  if (rfb) applyScaleMode(rfb, mode);
});

onBeforeUnmount(() => {
  destroyRfb();
});
</script>

<template>
  <article class="vnc-workspace flex flex-1 flex-col overflow-hidden">
    <div
      v-show="rfbConnected"
      class="vnc-toolbar"
    >
      <span
        class="vnc-toolbar-title"
        :title="desktopName || endpointLabel"
      >{{ desktopName || endpointLabel }}</span>
      <div class="vnc-toolbar-actions">
        <button
          type="button"
          class="ui-icon-button"
          :title="t('vncDesktop.ctrlAltDel')"
          :aria-label="t('vncDesktop.ctrlAltDel')"
          @click="sendCtrlAltDel"
        >
          <Keyboard
            :size="15"
            stroke-width="1.8"
          />
        </button>
        <button
          type="button"
          class="ui-icon-button"
          :title="t('vncDesktop.sendClipboard')"
          :aria-label="t('vncDesktop.sendClipboard')"
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
          :title="t('vncDesktop.scaleMode', { mode: scaleModeLabel })"
          :aria-label="t('vncDesktop.scaleMode', { mode: scaleModeLabel })"
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
          :title="t('vncDesktop.fullscreen')"
          :aria-label="t('vncDesktop.fullscreen')"
          @click="toggleFullscreen"
        >
          <Maximize
            :size="15"
            stroke-width="1.8"
          />
        </button>
      </div>
    </div>

    <section class="vnc-surface min-h-0 flex-1 overflow-hidden">
      <div
        ref="desktopMount"
        class="vnc-desktop-mount ui-fill-block ui-fill-inline"
      />

      <div
        v-if="overlayKind"
        class="vnc-overlay"
      >
        <form
          class="vnc-overlay-card"
          @submit.prevent="handleOverlaySubmit"
        >
          <span
            v-if="overlayKind === 'connecting'"
            class="vnc-spinner"
          />
          <h3 class="vnc-overlay-title">
            {{ overlayTitle }}
          </h3>
          <p
            v-if="overlayDescription"
            class="vnc-overlay-desc"
          >
            {{ overlayDescription }}
          </p>
          <input
            v-if="overlayKind === 'credentials' && credentialsPrompt.types.includes('username')"
            v-model="credentialUsername"
            class="ui-input ui-fill-inline"
            :placeholder="t('connectionDialog.fields.user')"
            autocomplete="off"
          >
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
            {{ overlayKind === "credentials" ? t("actions.connect") : t("actions.reconnect") }}
          </button>
        </form>
      </div>
    </section>
  </article>
</template>

<style lang="scss" scoped>
.vnc-workspace {
  position: relative;
  background: var(--bg-primary, #111);
}

.vnc-surface {
  position: relative;
}

.vnc-desktop-mount {
  position: absolute;
  inset: 0;
}

.vnc-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 10px;
  border-bottom: 1px solid var(--border-light);
  background: var(--bg-secondary, transparent);
  flex-shrink: 0;
}

.vnc-toolbar-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}

.vnc-toolbar-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.vnc-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in oklch, var(--bg-primary, #111) 72%, transparent);
  z-index: 10;
}

.vnc-overlay-card {
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

.vnc-overlay-title {
  margin: 0;
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--text-primary);
  text-align: center;
}

.vnc-overlay-desc {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--text-tertiary);
  text-align: center;
  word-break: break-all;
}

.vnc-spinner {
  align-self: center;
  width: 22px;
  height: 22px;
  border: 2px solid var(--border-light);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: vnc-spin 0.8s linear infinite;
}

@keyframes vnc-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
