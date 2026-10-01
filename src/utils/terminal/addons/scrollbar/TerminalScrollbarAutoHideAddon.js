const DEFAULT_IDLE_TIMEOUT_MS = 1200;
const HIDDEN_CLASS = "xterm-addon-scrollbar-idle-hidden";
const ACTIVITY_EVENTS = ["mousemove", "wheel", "mousedown", "click"];

function normalizeIdleTimeout(value) {
  const timeout = Number(value);
  return Number.isFinite(timeout) ? Math.max(0, Math.round(timeout)) : DEFAULT_IDLE_TIMEOUT_MS;
}

/**
 * Adds idle hiding for xterm.js' public DOM output without reaching into its
 * private services. The state is kept on the terminal root because xterm.js
 * rewrites the scrollbar node's className whenever its visibility changes.
 */
export class TerminalScrollbarAutoHideAddon {
  constructor({ idleTimeout = DEFAULT_IDLE_TIMEOUT_MS } = {}) {
    this._idleTimeout = normalizeIdleTimeout(idleTimeout);
    this._terminalElement = null;
    this._hideTimer = undefined;
    this._lastActivityAt = 0;
    this._isHidden = false;
    this._activityHandler = null;
  }

  activate(terminal) {
    this._terminalElement = terminal?.element || null;

    // The addon is loaded after terminal.open(). Keep it safe if a future
    // xterm.js version does not expose a root element at activation.
    if (!this._terminalElement) return;

    this._activityHandler = () => {
      this._revealAndScheduleHide();
    };

    for (const eventName of ACTIVITY_EVENTS) {
      this._terminalElement.addEventListener(eventName, this._activityHandler, {
        capture: true,
        passive: true,
      });
    }
    this._revealAndScheduleHide();
  }

  dispose() {
    this._clearHideTimer();
    if (this._terminalElement && this._activityHandler) {
      for (const eventName of ACTIVITY_EVENTS) {
        this._terminalElement.removeEventListener(eventName, this._activityHandler, true);
      }
    }
    if (this._isHidden) {
      this._terminalElement?.classList?.remove(HIDDEN_CLASS);
      this._isHidden = false;
    }
    this._activityHandler = null;
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

  _clearHideTimer() {
    if (this._hideTimer === undefined) return;
    clearTimeout(this._hideTimer);
    this._hideTimer = undefined;
  }
}
