import { getCurrentScope, onScopeDispose, watch } from "vue";
import {
  TERMINAL_CURSOR_WIDTH_MAX,
  TERMINAL_CURSOR_WIDTH_MIN,
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
  TERMINAL_SCROLLBACK_MAX,
  TERMINAL_SCROLLBACK_MIN,
  normalizeCursorInactiveStyle,
  normalizeCursorStyle,
  normalizeIntegerOption,
  normalizeNumberOption,
} from "../utils/terminalPanelHelpers";
import { terminalFontFamily } from "../utils/terminal/xtermOptions";

const TERMINAL_OPTION_WATCHERS = [
  ["terminalCursorBlink", "cursorBlink", Boolean, true, true],
  ["terminalCursorStyle", "cursorStyle", normalizeCursorStyle, true],
  ["terminalCursorInactiveStyle", "cursorInactiveStyle", normalizeCursorInactiveStyle, true],
  [
    "terminalCursorWidth",
    "cursorWidth",
    (value) =>
      normalizeIntegerOption(value, 1, TERMINAL_CURSOR_WIDTH_MIN, TERMINAL_CURSOR_WIDTH_MAX),
    true,
  ],
  [
    "terminalScrollSensitivity",
    "scrollSensitivity",
    (value) => normalizeNumberOption(value, 1, 0.1, 10),
  ],
  [
    "terminalFastScrollSensitivity",
    "fastScrollSensitivity",
    (value) => normalizeNumberOption(value, 5, 1, 20),
  ],
  [
    "terminalSmoothScrollDuration",
    "smoothScrollDuration",
    (value) => normalizeIntegerOption(value, 0, 0, 1000),
  ],
  ["terminalAltClickMovesCursor", "altClickMovesCursor", Boolean],
  ["terminalRightClickSelectsWord", "rightClickSelectsWord", Boolean],
  ["terminalScrollOnUserInput", "scrollOnUserInput", Boolean],
  ["terminalScrollOnEraseInDisplay", "scrollOnEraseInDisplay", Boolean],
  ["terminalDrawBoldTextInBrightColors", "drawBoldTextInBrightColors", Boolean, true],
  [
    "terminalMinimumContrastRatio",
    "minimumContrastRatio",
    (value) => normalizeNumberOption(value, 1, 1, 21),
    true,
  ],
  ["terminalCustomGlyphs", "customGlyphs", Boolean, true],
  ["terminalRescaleOverlappingGlyphs", "rescaleOverlappingGlyphs", Boolean, true],
  ["terminalMacOptionIsMeta", "macOptionIsMeta", Boolean],
  ["terminalMacOptionClickForcesSelection", "macOptionClickForcesSelection", Boolean],
];

export function registerTerminalOptionWatchers({
  getTerminal,
  getTheme,
  props,
  refitTerminalAfterFontMetricsChange,
  refreshTerminalViewport,
  syncTerminalRenderer,
  isForegroundRuntime,
}) {
  // 字体变化触发的图集重建 + refit 会产生完整重排；滚轮缩放时 fontSize 每个
  // tick 都变，逐次执行会明显卡顿。选项本身立即生效，这里只把重建尾随防抖
  // 到手势停止后做一次。
  const FONT_METRICS_SETTLE_MS = 100;
  let fontMetricsTimer = null;
  function scheduleFontMetricsSettle(terminal) {
    clearTimeout(fontMetricsTimer);
    fontMetricsTimer = setTimeout(() => {
      fontMetricsTimer = null;
      terminal.clearTextureAtlas?.();
      refitTerminalAfterFontMetricsChange();
    }, FONT_METRICS_SETTLE_MS);
  }
  if (getCurrentScope()) {
    onScopeDispose(() => clearTimeout(fontMetricsTimer));
  }

  watch(
    () => props.terminalTheme,
    () => {
      const terminal = getTerminal();
      if (!terminal) return;
      terminal.options.theme = getTheme();
      if (props.visible) {
        refreshTerminalViewport({ clearTextureAtlas: true });
      }
    },
  );

  watch(
    () => [props.terminalFontSize, props.terminalFontFamily, props.terminalLineHeight],
    ([fontSize, fontFamily, lineHeight]) => {
      const terminal = getTerminal();
      if (!terminal) return;
      terminal.options.fontSize = normalizeIntegerOption(
        fontSize,
        16,
        TERMINAL_FONT_SIZE_MIN,
        TERMINAL_FONT_SIZE_MAX,
      );
      terminal.options.fontFamily = terminalFontFamily(fontFamily);
      terminal.options.lineHeight = normalizeNumberOption(lineHeight, 1, 1, 2);
      scheduleFontMetricsSettle(terminal);
    },
  );

  watch(
    () => props.terminalScrollback,
    (value) => {
      const terminal = getTerminal();
      if (terminal) {
        terminal.options.scrollback = normalizeIntegerOption(
          value,
          9001,
          TERMINAL_SCROLLBACK_MIN,
          TERMINAL_SCROLLBACK_MAX,
        );
      }
    },
  );

  watch(
    () => TERMINAL_OPTION_WATCHERS.map(([propName]) => props[propName]),
    (values, previousValues = []) => {
      const terminal = getTerminal();
      if (!terminal) return;
      const options = {};
      let shouldRefresh = false;
      TERMINAL_OPTION_WATCHERS.forEach(
        ([_propName, optionName, normalize, refresh, foregroundOnly], index) => {
          const value = normalize ? normalize(values[index]) : values[index];
          options[optionName] = foregroundOnly ? value && isForegroundRuntime() : value;
          // 用归一化后的值比较：越界值被 clamp 到同一值时不应触发重绘。
          const previousValue = normalize
            ? normalize(previousValues[index])
            : previousValues[index];
          shouldRefresh ||= !!refresh && value !== previousValue;
        },
      );
      terminal.options = options;
      if (shouldRefresh && props.visible) {
        refreshTerminalViewport({ clearTextureAtlas: true });
      }
    },
  );

  watch(
    () => props.terminalWebgl,
    () => {
      syncTerminalRenderer();
    },
  );
}
