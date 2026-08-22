import { watch } from "vue";
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
  watch(
    () => props.terminalTheme,
    () => {
      const terminal = getTerminal();
      if (!terminal) return;
      terminal.options.theme = getTheme();
      if (props.visible) {
        refreshTerminalViewport();
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
      refitTerminalAfterFontMetricsChange();
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
          shouldRefresh ||= !!refresh && values[index] !== previousValues[index];
        },
      );
      terminal.options = options;
      if (shouldRefresh && props.visible) refreshTerminalViewport();
    },
  );

  watch(
    () => props.terminalWebgl,
    () => {
      syncTerminalRenderer();
    },
  );
}
