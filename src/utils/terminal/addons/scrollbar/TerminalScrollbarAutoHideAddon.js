const DEFAULT_IDLE_TIMEOUT_MS = 1200;
const HIDDEN_CLASS = "xterm-addon-scrollbar-idle-hidden";
const ACTIVITY_EVENTS = ["pointermove", "mousemove", "wheel", "mousedown", "click"];

function normalizeIdleTimeout(value) {
  const timeout = Number(value);
  return Number.isFinite(timeout) ? Math.max(0, Math.round(timeout)) : DEFAULT_IDLE_TIMEOUT_MS;
}

function pressKey(event) {
  return typeof event.pointerId === "number" ? event.pointerId : "mouse";
}

/**
 * Adds idle hiding for xterm.js' public DOM output without reaching into its
 * private services. The state is kept on the terminal root because xterm.js
 * rewrites the scrollbar node's className whenever its visibility changes.
 *
 * "Idle" is not just "no events": a held pointer must keep the scrollbar
 * visible even when no activity reaches the terminal root. Selection drags
 * leave the element bounds, and xterm's scrollbar thumb holds pointer capture
 * while stationary, so presses are tracked separately and releases are
 * observed on the window (the pointer may come up outside the element).
 */
export class TerminalScrollbarAutoHideAddon {
  constructor({ idleTimeout = DEFAULT_IDLE_TIMEOUT_MS } = {}) {
    this._idleTimeout = normalizeIdleTimeout(idleTimeout);
    this._terminalElement = null;
    this._releaseTarget = null;
    this._hideTimer = undefined;
    this._lastActivityAt = 0;
    this._isHidden = false;
    this._pressedPointers = new Set();
    this._pressStartEvents = [];
    this._pressEndEvents = [];
    this._activityHandler = null;
    this._pressStartHandler = null;
    this._pressEndHandler = null;
    this._windowBlurHandler = null;
    this._scrollDisposable = null;
  }

  activate(terminal) {
    this._terminalElement = terminal?.element || null;

    // The addon is loaded after terminal.open(). Keep it safe if a future
    // xterm.js version does not expose a root element at activation.
    if (!this._terminalElement) return;

    this._activityHandler = (event) => {
      this._reconcilePressedPointers(event);
      this._revealAndScheduleHide();
    };
    this._pressStartHandler = (event) => {
      this._pressedPointers.add(pressKey(event));
      this._revealAndScheduleHide();
    };
    this._pressEndHandler = (event) => {
      if (!this._pressedPointers.delete(pressKey(event))) return;
      if (this._pressedPointers.size === 0) this._revealAndScheduleHide();
    };
    this._windowBlurHandler = () => {
      if (this._pressedPointers.size === 0) return;
      this._pressedPointers.clear();
      this._scheduleHide();
    };

    for (const eventName of ACTIVITY_EVENTS) {
      this._terminalElement.addEventListener(eventName, this._activityHandler, {
        capture: true,
        passive: true,
      });
    }

    // Pointer Events unify mouse/pen/touch; the mouse fallback only matters
    // for WebViews old enough to lack them.
    const supportsPointerEvents = typeof PointerEvent !== "undefined";
    this._pressStartEvents = supportsPointerEvents ? ["pointerdown"] : ["mousedown"];
    this._pressEndEvents = supportsPointerEvents ? ["pointerup", "pointercancel"] : ["mouseup"];
    for (const eventName of this._pressStartEvents) {
      this._terminalElement.addEventListener(eventName, this._pressStartHandler, {
        capture: true,
        passive: true,
      });
    }
    this._releaseTarget = this._terminalElement.ownerDocument?.defaultView || this._terminalElement;
    for (const eventName of this._pressEndEvents) {
      this._releaseTarget.addEventListener(eventName, this._pressEndHandler, {
        capture: true,
        passive: true,
      });
    }
    this._releaseTarget.addEventListener("blur", this._windowBlurHandler, true);

    // Content scrolling (scrollbar drags, keyboard scrolls, touch momentum)
    // is activity even when no DOM event reaches the terminal root.
    this._scrollDisposable =
      typeof terminal?.onScroll === "function"
        ? terminal.onScroll(() => this._revealAndScheduleHide())
        : null;

    this._revealAndScheduleHide();
  }

  dispose() {
    this._clearHideTimer();
    if (this._terminalElement && this._activityHandler) {
      for (const eventName of ACTIVITY_EVENTS) {
        this._terminalElement.removeEventListener(eventName, this._activityHandler, true);
      }
      for (const eventName of this._pressStartEvents) {
        this._terminalElement.removeEventListener(eventName, this._pressStartHandler, true);
      }
    }
    if (this._releaseTarget && this._pressEndHandler) {
      for (const eventName of this._pressEndEvents) {
        this._releaseTarget.removeEventListener(eventName, this._pressEndHandler, true);
      }
      this._releaseTarget.removeEventListener("blur", this._windowBlurHandler, true);
    }
    this._scrollDisposable?.dispose();
    this._scrollDisposable = null;
    this._pressedPointers.clear();
    if (this._isHidden) {
      this._terminalElement?.classList?.remove(HIDDEN_CLASS);
      this._isHidden = false;
    }
    this._activityHandler = null;
    this._pressStartHandler = null;
    this._pressEndHandler = null;
    this._windowBlurHandler = null;
    this._pressStartEvents = [];
    this._pressEndEvents = [];
    this._releaseTarget = null;
    this._terminalElement = null;
  }

  _revealAndScheduleHide() {
    this._lastActivityAt = Date.now();
    if (this._isHidden) {
      this._terminalElement?.classList?.remove(HIDDEN_CLASS);
      this._isHidden = false;
    }
    this._scheduleHide();
  }

  _scheduleHide() {
    if (this._hideTimer !== undefined) return;
    this._hideTimer = setTimeout(() => this._hideWhenIdle(), this._idleTimeout);
  }

  _hideWhenIdle() {
    this._hideTimer = undefined;
    if (this._pressedPointers.size > 0) {
      // A held pointer with no activity is a stationary drag (e.g. a
      // selection held outside the window); keep probing instead of hiding.
      this._scheduleHide();
      return;
    }
    const elapsed = Date.now() - this._lastActivityAt;
    const remaining = this._idleTimeout - elapsed;
    if (remaining > 0) {
      this._hideTimer = setTimeout(() => this._hideWhenIdle(), remaining);
      return;
    }
    if (!this._isHidden) {
      this._terminalElement?.classList?.add(HIDDEN_CLASS);
      this._isHidden = true;
    }
  }

  // A pointer released outside the window never dispatches its release event
  // to us; the next move with no buttons pressed proves the drag is over.
  _reconcilePressedPointers(event) {
    if (this._pressedPointers.size === 0) return;
    if (event.type !== "pointermove" && event.type !== "mousemove") return;
    if (typeof event.buttons !== "number" || event.buttons !== 0) return;
    this._pressedPointers.clear();
  }

  _clearHideTimer() {
    if (this._hideTimer === undefined) return;
    clearTimeout(this._hideTimer);
    this._hideTimer = undefined;
  }
}
