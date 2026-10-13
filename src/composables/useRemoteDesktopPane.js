import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { useToasts } from "./useToasts";

const SCALE_MODES = ["fit", "none", "clip"];

// VNC（VncDesktopPane）与 RDP（RdpDesktopPane）桌面面板共用的协议无关壳层逻辑：
// 连接状态浮层（标题/描述/密码重试提交）、缩放模式本地覆盖循环、全屏切换、
// 本地剪贴板发送。协议差异全部经参数注入：
// - protocol（"vnc" / "rdp"）推导 i18n key 前缀（vncDesktop./rdpDesktop.、
//   terminal.connectionConnectingVnc/Rdp、connectionDialog.*.scaleModes）、
//   认证重试错误码（*_auth_required / *_auth_failed）与密码重试选项键（*Password）；
// - connected / sessionError 为协议层响应式状态（rfbConnected+rfbError /
//   bridgeConnected+bridgeError）；closedDescription 仅 RDP 提供（桥断开原因）；
// - credentialsPrompt + onCredentialsSubmit 仅 VNC 提供（noVNC 凭证请求弹层），
//   不提供时浮层永不进入 credentials 分支；
// - getCurrentScaleMode 给出协议层当前生效的缩放模式（VNC 读 rfb 实例标志位，
//   RDP 取 profile computed），返回 "" 表示当前不可切换（如 rfb 尚未建立）；
// - onScaleModeChange 把新生效的模式同步到协议层（VNC 写 rfb 实例；RDP 靠 CSS
//   类名响应 effectiveScaleMode，无需回调）。
export function useRemoteDesktopPane({
  protocol,
  props,
  emit,
  logger,
  connected,
  sessionError,
  credentialsPrompt = null,
  closedDescription = null,
  scaleMode,
  getCurrentScaleMode,
  onScaleModeChange = null,
  fullscreenTarget,
  canSendClipboard,
  sendClipboardText,
  onCredentialsSubmit = null,
}) {
  const { t, te } = useI18n();
  const { showToast } = useToasts();

  const passwordInput = ref("");
  const authRetryErrorCodes = new Set([`${protocol}_auth_required`, `${protocol}_auth_failed`]);
  const protocolTitle = `${protocol[0].toUpperCase()}${protocol.slice(1)}`;

  const connectionStatus = computed(() => props.connectionState?.status || "idle");
  const authRetryRequested = computed(() =>
    authRetryErrorCodes.has(props.connectionState?.error?.code || ""),
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

  // 单一状态浮层：优先级 凭证请求 > 协议层错误 > 连接中 > 失败 > 已断开。
  const overlayKind = computed(() => {
    if (credentialsPrompt?.value) return "credentials";
    if (sessionError.value) return "error";
    if (connectionStatus.value === "connecting") return "connecting";
    if (connectionStatus.value === "failed") return "failed";
    if (connectionStatus.value === "closed" && !connected.value) return "closed";
    return null;
  });

  const overlayTitle = computed(() => {
    switch (overlayKind.value) {
      case "credentials":
        return t(`${protocol}Desktop.credentialsTitle`);
      case "error":
        return t(`${protocol}Desktop.connectionError`);
      case "connecting":
        return t(`terminal.connectionConnecting${protocolTitle}`);
      case "failed":
        return failureLabel.value || t("connectionErrors.unknown");
      case "closed":
        return t(`${protocol}Desktop.sessionClosed`);
      default:
        return "";
    }
  });

  const overlayDescription = computed(() => {
    switch (overlayKind.value) {
      case "credentials":
        return t(`${protocol}Desktop.credentialsDescription`);
      case "error":
        return sessionError.value;
      case "connecting":
        return endpointLabel.value;
      case "failed":
        return failureDetail.value;
      case "closed":
        return closedDescription?.value || "";
      default:
        return "";
    }
  });

  const overlayNeedsPassword = computed(
    () =>
      overlayKind.value === "credentials" ||
      (overlayKind.value === "failed" && authRetryRequested.value),
  );

  const overlaySubmitDisabled = computed(() => overlayNeedsPassword.value && !passwordInput.value);

  function retryConnection(options = {}) {
    sessionError.value = "";
    emit("retryConnection", options);
  }

  function handleOverlaySubmit() {
    if (overlayKind.value === "credentials") {
      onCredentialsSubmit?.();
      return;
    }
    if (overlayKind.value === "failed" && authRetryRequested.value) {
      retryConnection({ [`${protocol}Password`]: passwordInput.value });
      passwordInput.value = "";
      return;
    }
    retryConnection();
  }

  // 工具条循环切换的本地缩放覆盖；profile 的 scaleMode 变化时清空回退。
  const scaleModeLocal = ref("");

  const effectiveScaleMode = computed(() => scaleModeLocal.value || scaleMode.value);

  const scaleModeLabel = computed(() =>
    t(`connectionDialog.${protocol}.scaleModes.${effectiveScaleMode.value}`),
  );

  function cycleScaleMode() {
    const current = scaleModeLocal.value || getCurrentScaleMode();
    if (!current) return;
    const next = SCALE_MODES[(SCALE_MODES.indexOf(current) + 1) % SCALE_MODES.length];
    scaleModeLocal.value = next;
    onScaleModeChange?.(next);
  }

  watch(scaleMode, (mode) => {
    scaleModeLocal.value = "";
    onScaleModeChange?.(mode);
  });

  function toggleFullscreen() {
    const target = fullscreenTarget.value;
    if (!target) return;
    if (document.fullscreenElement) {
      void document.exitFullscreen?.();
    } else {
      void target.requestFullscreen?.();
    }
  }

  async function sendLocalClipboard() {
    if (!canSendClipboard()) return;
    try {
      const text = await readText();
      if (text) sendClipboardText(text);
    } catch (error) {
      logger.warn("clipboard.read.failed", error);
      showToast({ type: "error", title: t(`${protocol}Desktop.clipboardSendFailed`) });
    }
  }

  return {
    passwordInput,
    connectionStatus,
    endpointLabel,
    overlayKind,
    overlayTitle,
    overlayDescription,
    overlayNeedsPassword,
    overlaySubmitDisabled,
    handleOverlaySubmit,
    effectiveScaleMode,
    scaleModeLabel,
    cycleScaleMode,
    toggleFullscreen,
    sendLocalClipboard,
  };
}
