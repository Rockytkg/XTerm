import { ref } from "vue";
import { createRuntimeId } from "../utils/runtimeIds.js";

const toasts = ref([]);
const dismissTimers = new Map();
const removalTimers = new Map();

function clearTimer(map, id) {
  const timer = map.get(id);
  if (timer) window.clearTimeout(timer);
  map.delete(id);
}

function clearToastDismissTimer(id) {
  clearTimer(dismissTimers, id);
}

function clearToastRemovalTimer(id) {
  clearTimer(removalTimers, id);
}

function scheduleDismiss(id, duration) {
  clearToastDismissTimer(id);
  if (!Number.isFinite(duration) || duration <= 0) return;
  dismissTimers.set(
    id,
    window.setTimeout(() => dismissToast(id), duration),
  );
}

function resolveDuration(options) {
  return options.duration ?? (options.type === "loading" ? 600_000 : 3_200);
}

export function showToast(options) {
  const id = options.id || createRuntimeId();
  clearToastRemovalTimer(id);
  const toast = {
    id,
    open: true,
    type: options.type || "info",
    title: options.title || "",
    message: options.message || "",
    duration: resolveDuration(options),
  };

  toasts.value = [...toasts.value.filter((item) => item.id !== id), toast];
  scheduleDismiss(id, toast.duration);
  return id;
}

function updateToast(id, patch) {
  let nextToast = null;
  toasts.value = toasts.value.map((toast) => {
    if (toast.id !== id) return toast;
    nextToast = { ...toast, ...patch, open: true };
    // 时长不能沿用旧值：loading 的 10 分钟兜底时长会粘到更新后的
    // success/error 上，导致结果提示迟迟不消失；除非 patch 显式给时长。
    nextToast.duration = resolveDuration({ type: nextToast.type, duration: patch.duration });
    return nextToast;
  });

  if (nextToast) {
    clearToastRemovalTimer(id);
    scheduleDismiss(id, nextToast.duration);
    return;
  }
  // 原 toast 已被用户手动关闭并移出列表：结果提示（成功/失败）不能因此丢失，
  // 按 patch 内容重建一条新提示。
  showToast({ id, ...patch });
}

function dismissToast(id) {
  clearToastDismissTimer(id);
  const hadToast = toasts.value.some((toast) => toast.id === id);
  if (!hadToast) {
    clearToastRemovalTimer(id);
    return;
  }
  toasts.value = toasts.value.map((toast) => (toast.id === id ? { ...toast, open: false } : toast));
  clearToastRemovalTimer(id);
  removalTimers.set(
    id,
    window.setTimeout(() => {
      removalTimers.delete(id);
      toasts.value = toasts.value.filter((toast) => toast.id !== id);
    }, 180),
  );
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    for (const timer of dismissTimers.values()) window.clearTimeout(timer);
    for (const timer of removalTimers.values()) window.clearTimeout(timer);
    dismissTimers.clear();
    removalTimers.clear();
    toasts.value = [];
  });
}

export function useToasts() {
  return {
    dismissToast,
    showToast,
    toasts,
    updateToast,
  };
}
