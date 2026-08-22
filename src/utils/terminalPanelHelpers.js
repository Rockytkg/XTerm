export const TERMINAL_OUTPUT_FLUSH_MS = 8;
export const TERMINAL_OUTPUT_FLUSH_MAX_CHARS = 96 * 1024;
export const TERMINAL_OUTPUT_WRITE_CHUNK_CHARS = 32 * 1024;
export const TERMINAL_OUTPUT_BACKPRESSURE_HIGH_WATERMARK = 6;
export const TERMINAL_OUTPUT_BACKPRESSURE_LOW_WATERMARK = 3;
export const TERMINAL_FONT_SIZE_MIN = 8;
export const TERMINAL_FONT_SIZE_MAX = 36;
export const TERMINAL_SCROLLBACK_MIN = 100;
export const TERMINAL_SCROLLBACK_MAX = 30000;
export const TERMINAL_CURSOR_WIDTH_MIN = 1;
export const TERMINAL_CURSOR_WIDTH_MAX = 10;
export const HIGHLIGHT_MATCH_TEXT = "text";
const HIGHLIGHT_MATCH_REGEX = "regex";
const TERMINAL_CURSOR_STYLES = new Set(["block", "underline", "bar"]);
const TERMINAL_CURSOR_INACTIVE_STYLES = new Set(["outline", "block", "bar", "underline", "none"]);

export function normalizeHexColor(value, fallback = "") {
  const color = String(value || "").trim();
  return /^#[0-9a-fA-F]{6}$/.test(color) ? color : fallback;
}

export function normalizeHighlightMatchType(value) {
  return value === HIGHLIGHT_MATCH_REGEX ? HIGHLIGHT_MATCH_REGEX : HIGHLIGHT_MATCH_TEXT;
}

export function normalizeNumberOption(value, fallback, min, max) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return fallback;
  }
  return Math.min(max, Math.max(min, number));
}

export function normalizeIntegerOption(value, fallback, min, max) {
  return Math.round(normalizeNumberOption(value, fallback, min, max));
}

export function normalizeCursorStyle(value) {
  return TERMINAL_CURSOR_STYLES.has(value) ? value : "block";
}

export function normalizeCursorInactiveStyle(value) {
  return TERMINAL_CURSOR_INACTIVE_STYLES.has(value) ? value : "outline";
}

export function fg(color) {
  const red = parseInt(color.slice(1, 3), 16);
  const green = parseInt(color.slice(3, 5), 16);
  const blue = parseInt(color.slice(5, 7), 16);
  return `\x1b[38;2;${red};${green};${blue}m`;
}
