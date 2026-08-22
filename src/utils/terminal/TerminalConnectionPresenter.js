export class TerminalConnectionPresenter {
  constructor({ dropOutput, getState, handleStatus, onConnecting, resetStatus }) {
    this.dropOutput = dropOutput;
    this.getState = getState;
    this.handleStatus = handleStatus;
    this.onConnecting = onConnecting;
    this.resetStatus = resetStatus;
  }

  replay() {
    const state = this.getState?.() || {};
    const status = state.status || "idle";
    this.handleStatus?.(status, state.phase || null);
    if (status === "connecting") this.onConnecting?.();
  }

  reset({ replay = false } = {}) {
    // Output and status fingerprints must be invalidated together; otherwise
    // a discarded status write can be mistaken for an already rendered one.
    this.dropOutput?.();
    this.resetStatus?.();
    if (replay) this.replay();
  }

  resetBackendSession({ preserveViewport = false } = {}) {
    // A preserved Telnet session can fail while its backend id is being
    // detached. The shared output queue may already contain the failure
    // presentation, so neither output nor status state may be reset here.
    if (preserveViewport) return;
    this.reset({ replay: true });
  }
}
