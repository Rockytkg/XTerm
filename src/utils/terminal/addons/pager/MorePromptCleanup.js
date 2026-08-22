const CONTINUE_INPUT = " ";
const LOCAL_CLEAR_PROMPT = "\r\x1b[2K";
const MORE_PROMPT_LINE_PATTERN = /----\s*More\s*----\s*$/u;
const MORE_PROMPT_TEXT_PATTERN = /----\s*More\s*----/u;
const MORE_PROMPT_ARTIFACT_PATTERN = /----\s*More\s*----[\t \0]*/u;
const LEADING_FILL_PATTERN = /^[\t \0]+/u;

export class MorePromptCleanup {
  constructor({ isEnabled, readCurrentLine, queueLocalClear }) {
    this._isEnabled = isEnabled;
    this._readCurrentLine = readCurrentLine;
    this._queueLocalClear = queueLocalClear;
    this.reset();
  }

  reset() {
    this._promptCredits = 0;
    this._fillCredits = 0;
  }

  observeInput(data) {
    if (!this._isEnabled?.()) {
      this.reset();
      return;
    }
    if (data !== CONTINUE_INPUT) return;

    if (MORE_PROMPT_LINE_PATTERN.test(this._readCurrentLine?.() || "")) {
      // Queue the local erase through the same output pipeline as backend data.
      // xterm parsing, render events, and highlight refresh then observe a
      // single ordered stream instead of racing a direct terminal.write().
      this._queueLocalClear?.(LOCAL_CLEAR_PROMPT);
      this._fillCredits += 1;
      return;
    }

    if (!this._hasCredits()) return;
    // A rapid extra space can be sent before the remote paints its next pager
    // prompt. Record one future prompt/fill pair so output cleanup remains
    // deterministic regardless of key repeat cadence.
    this._promptCredits += 1;
    this._fillCredits += 1;
  }

  cleanOutput(data) {
    if (!this._isEnabled?.()) {
      this.reset();
      return data;
    }
    if (!data || !this._hasCredits()) return data;

    let cleaned = this._promptCredits > 0 ? data : this._consumeFill(data);
    while (this._promptCredits > 0) {
      const match = MORE_PROMPT_ARTIFACT_PATTERN.exec(cleaned);
      if (!match) break;
      const artifact = match[0];
      const promptText = MORE_PROMPT_TEXT_PATTERN.exec(artifact)?.[0] || artifact;
      const before = cleaned.slice(0, match.index);
      const after = cleaned.slice(match.index + artifact.length);
      cleaned = before + after;
      this._promptCredits -= 1;
      if (this._fillCredits > 0 && (artifact.length > promptText.length || after.length > 0)) {
        this._fillCredits -= 1;
      }
    }
    if (this._promptCredits === 0) cleaned = this._consumeFill(cleaned);
    return cleaned;
  }

  _hasCredits() {
    return this._promptCredits > 0 || this._fillCredits > 0;
  }

  _consumeFill(data) {
    if (!data || this._fillCredits <= 0) return data;
    const withoutLeadingFill = data.replace(LEADING_FILL_PATTERN, "");
    if (!withoutLeadingFill) return "";
    this._fillCredits -= 1;
    return withoutLeadingFill;
  }
}
