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
  // drawBoldTextInBrightColors / minimumContrastRatio / rescaleOverlappingGlyphs
  // 由 xterm 的 RenderService 在选项变化时 clear + handleResize + 全刷新，
  // 图集也随配置自动重建，应用侧无需（也不应）再刷一次。
  ["terminalDrawBoldTextInBrightColors", "drawBoldTextInBrightColors", Boolean],
  [
    "terminalMinimumContrastRatio",
    "minimumContrastRatio",
    (value) => normalizeNumberOption(value, 1, 1, 21),
  ],
  ["terminalRescaleOverlappingGlyphs", "rescaleOverlappingGlyphs", Boolean],
  ["terminalMacOptionIsMeta", "macOptionIsMeta", Boolean],
  ["terminalMacOptionClickForcesSelection", "macOptionClickForcesSelection", Boolean],
];

export function registerTerminalOptionWatchers({
  getTerminal,
  getTheme,
  props,
  refitTerminalAfterFontMetricsChange,
  refreshTerminalViewport,
  reloadWebglRenderer,
  syncTerminalRenderer,
  isForegroundRuntime,
}) {
  // 字体变化触发的图集重建 + refit 会产生完整重排；滚轮缩放时 fontSize 每个
  // tick 都变，逐次执行会明显卡顿。选项本身立即生效，这里只把重建尾随防抖
  // 到手势停止后做一次。图集由 xterm 在字符尺寸/字体配置变化时自行重建，
  // 这里再 clearTextureAtlas 只会擦掉刚预热的字形、触发一次整体重光栅。
  const FONT_METRICS_SETTLE_MS = 100;
  let fontMetricsTimer = null;
  function scheduleFontMetricsSettle() {
    clearTimeout(fontMetricsTimer);
    fontMetricsTimer = setTimeout(() => {
      fontMetricsTimer = null;
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
      // xterm 在 theme 变化时会重建字符图集（主题色是图集配置的一部分）并触发
      // 全视口刷新；项目侧再 clearTextureAtlas + refresh 只是白做一次
      // _clearModel + 全屏重绘，还会丢掉 idle 预热的字形。
      terminal.options.theme = getTheme();
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
      scheduleFontMetricsSettle();
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
        // 只刷新视口，不清图集：字形相关选项已经由 xterm 自己重建图集。
        refreshTerminalViewport();
      }
    },
  );

  watch(
    () => props.terminalWebgl,
    () => {
      syncTerminalRenderer();
    },
  );

  watch(
    () => props.terminalCustomGlyphs,
    (value, previousValue) => {
      if (Boolean(value) === Boolean(previousValue)) return;
      void reloadWebglRenderer();
    },
  );
}
