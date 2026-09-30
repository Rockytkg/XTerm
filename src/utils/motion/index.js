import { ref } from "vue";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

let motionPreferenceEnabled = true;

// 模块加载即初始化 matchMedia：若惰性到首次读取才创建，下面的 change 监听器
// 会在查询对象存在之前执行注册，系统级 reduced-motion 切换将永远收不到回调。
// 非浏览器环境（node --test 单测）无 window/document，全部按 null 回退。
const reducedMotionQuery =
  typeof window === "undefined" ? null : (window.matchMedia?.(REDUCED_MOTION_QUERY) ?? null);

// 响应式镜像：偏好/系统设置变化时写入，computed/watch 读取 motionEnabled()
// 才能获得重算依赖（直接读模块级布尔值不会被 Vue 追踪）。
const motionActive = ref(true);

function prefersReducedMotion() {
  return !!reducedMotionQuery?.matches;
}

function setRootMotionState() {
  const enabled = motionPreferenceEnabled && !prefersReducedMotion();
  if (typeof document !== "undefined") {
    document.documentElement.dataset.motion = enabled ? "on" : "off";
  }
  motionActive.value = enabled;
}

export function setMotionPreferenceEnabled(enabled) {
  motionPreferenceEnabled = !!enabled;
  setRootMotionState();
}

export function motionEnabled({ disabled = false } = {}) {
  return !disabled && motionActive.value;
}

function readMotionToken(name, fallback) {
  if (typeof getComputedStyle !== "function" || typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

export function readMotionDurationMs(name, fallbackMs) {
  const value = readMotionToken(name, "");
  const match = /^([\d.]+)(ms|s)$/.exec(value);
  if (!match) return fallbackMs;
  const amount = Number.parseFloat(match[1]);
  return match[2] === "s" ? amount * 1000 : amount;
}

// sortablejs 的拖拽位移时长/缓动以 CSS motion token（styles/tokens.scss）为
// 单一来源，运行时读取；token 调整后无需同步 JS 常量。
export function getSortableMotion() {
  return {
    animation: readMotionDurationMs("--motion-duration-base", 110),
    easing: readMotionToken("--motion-ease", "cubic-bezier(0.25, 0.1, 0.25, 1)"),
  };
}

export async function runViewTransition(update, { className, disabled = false } = {}) {
  const root = document.documentElement;
  const canTransition =
    motionEnabled({ disabled }) && typeof document.startViewTransition === "function";

  if (!canTransition) {
    await update();
    return false;
  }

  if (className) root.classList.add(className);
  try {
    const transition = document.startViewTransition(update);
    await transition.finished;
    return true;
  } finally {
    if (className) root.classList.remove(className);
  }
}

setRootMotionState();
reducedMotionQuery?.addEventListener?.("change", setRootMotionState);
