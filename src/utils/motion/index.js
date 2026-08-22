import { gsap } from "gsap";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
const transitionTweens = new WeakMap();

let motionPreferenceEnabled = true;
let reducedMotionQuery = null;

const EASE = {
  standard: "power3.out",
  exit: "power2.in",
};

const DURATION = {
  quick: 0.07,
  base: 0.11,
  slow: 0.18,
};

export const sortableMotion = {
  animation: 120,
  easing: "cubic-bezier(0.25, 0.1, 0.25, 1)",
};

const PANEL_PRESENCE = presence({
  y: 5,
  closeY: -2,
  scale: 1,
  openDuration: DURATION.base,
  closedDuration: DURATION.quick,
});

gsap.defaults({
  ease: EASE.standard,
  overwrite: "auto",
});

function prefersReducedMotion() {
  reducedMotionQuery ??= window.matchMedia?.(REDUCED_MOTION_QUERY) ?? null;
  return !!reducedMotionQuery?.matches;
}

function presence({ y, closeY = 3, scale, openDuration, closedDuration }) {
  return {
    from: vars({ alpha: 0, y, scale }),
    open: vars({ alpha: 1, y: 0, scale: 1, duration: openDuration, ease: EASE.standard }),
    closed: vars({
      alpha: 0,
      y: closeY,
      scale,
      duration: closedDuration,
      ease: EASE.exit,
    }),
  };
}

function vars({ alpha, y, scale, duration, ease }) {
  const result = {};
  if (alpha !== undefined) result.autoAlpha = alpha;
  if (y !== undefined) result["--motion-y"] = `${y}px`;
  if (scale !== undefined) result["--motion-scale"] = scale;
  if (duration !== undefined) result.duration = duration;
  if (ease !== undefined) result.ease = ease;
  return result;
}

function setRootMotionState() {
  document.documentElement.dataset.motion = motionEnabled() ? "on" : "off";
}

export function setMotionPreferenceEnabled(enabled) {
  motionPreferenceEnabled = !!enabled;
  setRootMotionState();
}

export function motionEnabled({ disabled = false } = {}) {
  return !disabled && motionPreferenceEnabled && !prefersReducedMotion();
}

function stopTweens(el) {
  transitionTweens.get(el)?.kill();
  transitionTweens.delete(el);
  gsap.killTweensOf(el);
}

function clearMotion(el) {
  stopTweens(el);
  gsap.set(el, { clearProps: "all" });
}

function finishNow(done) {
  done();
}

function tweenTransition(el, state, done, { clearAfter = false } = {}) {
  stopTweens(el);
  const tween = gsap.to(el, {
    ...PANEL_PRESENCE[state],
    onComplete: () => {
      transitionTweens.delete(el);
      if (clearAfter) gsap.set(el, { clearProps: "all" });
      done();
    },
  });
  transitionTweens.set(el, tween);
}

export function createPanelTransitionHooks({ disabled = () => false } = {}) {
  const canAnimate = () => motionEnabled({ disabled: disabled() });

  return {
    css: false,
    beforeEnter(el) {
      stopTweens(el);
      if (canAnimate()) gsap.set(el, PANEL_PRESENCE.from);
    },
    enter(el, done) {
      if (!canAnimate()) {
        clearMotion(el);
        finishNow(done);
        return;
      }
      tweenTransition(el, "open", done, { clearAfter: true });
    },
    leave(el, done) {
      if (!canAnimate()) {
        stopTweens(el);
        finishNow(done);
        return;
      }
      tweenTransition(el, "closed", done);
    },
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
