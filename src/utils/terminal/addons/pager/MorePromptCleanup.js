const CONTINUE_INPUT = " ";
const LOCAL_CLEAR_PROMPT = "\r\x1b[2K";
const MORE_PROMPT_LINE_PATTERN = /----\s*More\s*----\s*$/u;
const MORE_PROMPT_TEXT_PATTERN = /----\s*More\s*----/u;
const MORE_PROMPT_ARTIFACT_PATTERN = /----\s*More\s*----[\t \0]*/u;
const LEADING_FILL_PATTERN = /^[\t \0]+/u;
// 预支的 credit 只在"远端即将重绘 pager"的有效窗口内成立；远端没画 pager
// （用户按 q 退出后又敲空格等）时，过期 credit 会把后续正常输出里的
// `---- More ----` 文本或前导填充误删。
const CREDIT_TTL_MS = 10_000;

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
    this._creditsExpireAt = 0;
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
      this._creditsExpireAt = Date.now() + CREDIT_TTL_MS;
      return;
    }

    if (!this._hasCredits()) return;
    // A rapid extra space can be sent before the remote paints its next pager
    // prompt. Record one future prompt/fill pair so output cleanup remains
    // deterministic regardless of key repeat cadence.
    this._promptCredits += 1;
    this._fillCredits += 1;
    this._creditsExpireAt = Date.now() + CREDIT_TTL_MS;
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
    if (this._creditsExpireAt && Date.now() > this._creditsExpireAt) {
      this._promptCredits = 0;
      this._fillCredits = 0;
      this._creditsExpireAt = 0;
    }
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
