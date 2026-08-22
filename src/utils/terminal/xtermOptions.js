import { getTerminalTheme } from "../terminalColors";
import {
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
  TERMINAL_SCROLLBACK_MAX,
  TERMINAL_SCROLLBACK_MIN,
  normalizeCursorInactiveStyle,
  normalizeCursorStyle,
  normalizeIntegerOption,
  normalizeNumberOption,
} from "../terminalPanelHelpers";

const TERMINAL_FONT_FALLBACKS = [
  "Cascadia Code",
  "JetBrains Mono",
  "Fira Code",
  "Consolas",
  "Menlo",
  "DejaVu Sans Mono",
  "Liberation Mono",
  "monospace",
];

export function createXtermOptions(props, isForegroundRuntime) {
  return {
    allowProposedApi: true,
    allowTransparency: false,
    altClickMovesCursor: props.terminalAltClickMovesCursor,
    convertEol: false,
    cursorBlink: props.terminalCursorBlink && isForegroundRuntime,
    cursorInactiveStyle: normalizeCursorInactiveStyle(props.terminalCursorInactiveStyle),
    cursorStyle: normalizeCursorStyle(props.terminalCursorStyle),
    cursorWidth: normalizeIntegerOption(props.terminalCursorWidth, 1, 1, 10),
    customGlyphs: props.terminalCustomGlyphs,
    drawBoldTextInBrightColors: props.terminalDrawBoldTextInBrightColors,
    fastScrollSensitivity: normalizeNumberOption(props.terminalFastScrollSensitivity, 5, 1, 20),
    fontFamily: terminalFontFamily(props.terminalFontFamily),
    fontSize: normalizeIntegerOption(
      props.terminalFontSize,
      16,
      TERMINAL_FONT_SIZE_MIN,
      TERMINAL_FONT_SIZE_MAX,
    ),
    letterSpacing: 0,
    lineHeight: normalizeNumberOption(props.terminalLineHeight, 1, 1, 2),
    macOptionClickForcesSelection: props.terminalMacOptionClickForcesSelection,
    macOptionIsMeta: props.terminalMacOptionIsMeta,
    minimumContrastRatio: normalizeNumberOption(props.terminalMinimumContrastRatio, 1, 1, 21),
    rescaleOverlappingGlyphs: props.terminalRescaleOverlappingGlyphs,
    rightClickSelectsWord: props.terminalRightClickSelectsWord,
    scrollback: normalizeIntegerOption(
      props.terminalScrollback,
      9001,
      TERMINAL_SCROLLBACK_MIN,
      TERMINAL_SCROLLBACK_MAX,
    ),
    scrollOnEraseInDisplay: props.terminalScrollOnEraseInDisplay,
    scrollOnUserInput: props.terminalScrollOnUserInput,
    scrollSensitivity: normalizeNumberOption(props.terminalScrollSensitivity, 1, 0.1, 10),
    smoothScrollDuration: normalizeIntegerOption(props.terminalSmoothScrollDuration, 0, 0, 1000),
    theme: getTerminalTheme(props.terminalTheme),
    windowOptions: {
      getCellSizePixels: false,
      getWinSizeChars: false,
      getWinSizePixels: false,
    },
  };
}

export function terminalFontFamily(fontFamily) {
  return [fontFamily, ...TERMINAL_FONT_FALLBACKS]
    .filter(Boolean)
    .map((name) => (name === "monospace" ? name : `"${name}"`))
    .join(", ");
}
