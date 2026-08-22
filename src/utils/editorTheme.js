import { EditorView } from "@codemirror/view";
import { githubDarkInit, githubLightInit } from "@uiw/codemirror-theme-github";

const EDITOR_THEME_MODES = ["follow", "light", "dark"];

export function normalizeEditorThemeMode(value) {
  return EDITOR_THEME_MODES.includes(value) ? value : "follow";
}

export function resolveEditorTheme(mode, appTheme) {
  const normalizedMode = normalizeEditorThemeMode(mode);
  if (normalizedMode === "light" || normalizedMode === "dark") return normalizedMode;
  return appTheme === "dark" ? "dark" : "light";
}

// 字体与字号由 .sftp-editor 上的 --sftp-editor-font-family / --sftp-editor-font-size
// CSS 变量统一驱动（见 styles/sftp.scss）。fontFamily 必须显式传给主题：createTheme
// 会把未提供的 fontFamily 默认值 monospace 直接写到 .cm-content/.cm-line/.cm-gutters
// 等无层规则上，优先级高于项目 @layer 内的 SCSS，会覆盖用户设置的字体。
const packagedThemes = {
  light: githubLightInit({
    settings: {
      background: "var(--bg-terminal)",
      caret: "var(--text-terminal)",
      fontFamily: "var(--sftp-editor-font-family)",
      foreground: "var(--text-terminal)",
      gutterBackground: "color-mix(in oklch, var(--bg-terminal) 82%, var(--bg-primary))",
      gutterForeground: "var(--text-terminal-dim)",
      lineHighlight: "color-mix(in oklch, var(--accent-light) 34%, transparent)",
      selection: "color-mix(in oklch, var(--accent) 28%, transparent)",
      selectionMatch: "color-mix(in oklch, var(--warning) 32%, transparent)",
    },
  }),
  dark: githubDarkInit({
    settings: {
      background: "var(--bg-terminal)",
      caret: "var(--text-terminal)",
      fontFamily: "var(--sftp-editor-font-family)",
      foreground: "var(--text-terminal)",
      gutterBackground: "color-mix(in oklch, var(--bg-terminal) 82%, var(--bg-primary))",
      gutterForeground: "var(--text-terminal-dim)",
      lineHighlight: "color-mix(in oklch, var(--accent-light) 34%, transparent)",
      selection: "color-mix(in oklch, var(--accent) 32%, transparent)",
      selectionMatch: "color-mix(in oklch, var(--warning) 30%, transparent)",
    },
  }),
};

export function editorThemeExtension(theme) {
  const resolved = theme === "dark" ? "dark" : "light";
  return packagedThemes[resolved];
}

// 编辑器静态外观（面板、搜索框、选区、括号匹配、诊断波浪线）。
// 这些样式必须与 CodeMirror 自身注入的基础主题同级竞争（项目 SCSS 在 @layer 内，
// 优先级低于 CM 注入的无层样式），因此留在 EditorView.theme 中；作为模块级常量
// 只创建一次，字号缩放等动态变化不会触发整块主题重建。
export const editorChromeTheme = EditorView.theme({
  "&": {
    backgroundColor: "var(--bg-terminal)",
    color: "var(--text-terminal)",
  },
  ".cm-content": {
    caretColor: "var(--text-terminal)",
  },
  ".cm-cursor, .cm-dropCursor": {
    borderLeftColor: "var(--text-terminal)",
  },
  ".cm-gutters": {
    borderRight: "1px solid var(--border-light)",
    backgroundColor: "color-mix(in oklch, var(--bg-terminal) 82%, var(--bg-primary))",
    color: "var(--text-terminal-dim)",
  },
  ".cm-activeLine, .cm-activeLineGutter": {
    backgroundColor: "color-mix(in oklch, var(--accent-light) 34%, transparent)",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "color-mix(in oklch, var(--accent) 28%, transparent)",
  },
  ".cm-matchingBracket": {
    backgroundColor: "color-mix(in oklch, var(--success-bg) 72%, transparent)",
    outline: "1px solid color-mix(in oklch, var(--success) 36%, transparent)",
  },
  ".cm-nonmatchingBracket": {
    backgroundColor: "color-mix(in oklch, var(--danger-bg) 72%, transparent)",
    color: "var(--danger)",
    outline: "1px solid color-mix(in oklch, var(--danger) 42%, transparent)",
  },
  ".cm-diagnostic": {
    textDecorationColor: "var(--danger)",
  },
  ".cm-panels": {
    border: "0",
    backgroundColor: "transparent",
    color: "var(--text-primary)",
    fontFamily: "var(--font-sans)",
  },
  ".cm-panel.cm-search": {
    position: "relative",
    display: "grid",
    alignItems: "center",
    gridTemplateColumns: "minmax(260px, 1fr) repeat(3, max-content) repeat(3, max-content) 32px",
    gridAutoRows: "32px",
    gap: "8px",
    padding: "10px",
    borderBottom: "1px solid var(--border-light)",
    backgroundColor: "color-mix(in oklch, var(--bg-secondary) 86%, var(--bg-terminal))",
    boxShadow:
      "inset 0 1px 0 color-mix(in oklch, var(--text-primary) 4%, transparent), 0 8px 18px color-mix(in oklch, var(--bg-primary) 24%, transparent)",
    color: "var(--text-primary)",
    fontFamily: "inherit",
    fontSize: "12px",
    lineHeight: "1",
  },
  ".cm-panel.cm-search br": {
    display: "none",
  },
  ".cm-panel.cm-search label": {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "7px",
    height: "32px",
    padding: "0 9px",
    border: "1px solid var(--border-light)",
    borderRadius: "7px",
    backgroundColor: "color-mix(in oklch, var(--bg-primary) 80%, var(--bg-secondary))",
    color: "var(--text-secondary)",
    fontSize: "12px",
    fontWeight: "600",
    whiteSpace: "nowrap",
    userSelect: "none",
    boxSizing: "border-box",
    transition:
      "border-color var(--motion-duration-quick) var(--ease-default), background-color var(--motion-duration-quick) var(--ease-default), color var(--motion-duration-quick) var(--ease-default)",
  },
  ".cm-panel.cm-search label:hover": {
    borderColor: "color-mix(in oklch, var(--accent) 38%, var(--border))",
    color: "var(--text-primary)",
  },
  ".cm-panel.cm-search label:has(input:checked)": {
    borderColor: "color-mix(in oklch, var(--accent) 54%, var(--border-light))",
    backgroundColor: "color-mix(in oklch, var(--accent-light) 48%, var(--bg-primary))",
    color: "var(--accent)",
  },
  ".cm-panel.cm-search .cm-textfield": {
    appearance: "none",
    boxSizing: "border-box",
    width: "100%",
    height: "32px",
    minWidth: "0",
    margin: "0",
    padding: "0 11px",
    border: "1px solid var(--border-light)",
    borderRadius: "7px",
    outline: "none",
    backgroundColor: "color-mix(in oklch, var(--bg-primary) 94%, transparent)",
    backgroundImage: "none",
    boxShadow:
      "inset 0 1px 2px color-mix(in oklch, var(--bg-primary) 18%, transparent), 0 1px 0 color-mix(in oklch, var(--text-primary) 4%, transparent)",
    color: "var(--text-primary)",
    fontFamily: "inherit",
    fontSize: "12px",
    fontWeight: "500",
    lineHeight: "32px",
    transition:
      "border-color var(--motion-duration-quick) var(--ease-default), box-shadow var(--motion-duration-quick) var(--ease-default), background-color var(--motion-duration-quick) var(--ease-default)",
  },
  ".cm-panel.cm-search .cm-textfield::placeholder": {
    color: "var(--text-tertiary)",
    opacity: "1",
  },
  ".cm-panel.cm-search .cm-textfield:focus": {
    borderColor: "var(--accent)",
    backgroundColor: "color-mix(in oklch, var(--bg-primary) 90%, var(--accent-light))",
    boxShadow: "0 0 0 2px color-mix(in oklch, var(--accent) 24%, transparent)",
  },
  ".cm-panel.cm-search input[name='search']": {
    gridColumn: "1",
    gridRow: "1",
  },
  ".cm-panel.cm-search input[name='replace']": {
    gridColumn: "1",
    gridRow: "2",
  },
  ".cm-panel.cm-search input[type='checkbox']": {
    appearance: "none",
    display: "grid",
    placeContent: "center",
    width: "14px",
    height: "14px",
    margin: "0",
    border: "1px solid var(--border)",
    borderRadius: "4px",
    backgroundColor: "var(--bg-secondary)",
    accentColor: "var(--accent)",
    flex: "0 0 auto",
  },
  ".cm-panel.cm-search input[type='checkbox']::before": {
    content: "''",
    width: "7px",
    height: "7px",
    borderRadius: "2px",
    transform: "scale(0)",
    backgroundColor: "var(--accent)",
    transition: "transform var(--motion-duration-quick) var(--ease-default)",
  },
  ".cm-panel.cm-search input[type='checkbox']:checked": {
    borderColor: "color-mix(in oklch, var(--accent) 72%, var(--border))",
    backgroundColor: "color-mix(in oklch, var(--accent-light) 55%, var(--bg-secondary))",
  },
  ".cm-panel.cm-search input[type='checkbox']:checked::before": {
    transform: "scale(1)",
  },
  ".cm-panel.cm-search input[type='checkbox']:focus-visible": {
    outline: "2px solid color-mix(in oklch, var(--accent) 42%, transparent)",
    outlineOffset: "2px",
  },
  ".cm-panel.cm-search button": {
    appearance: "none",
    boxSizing: "border-box",
    height: "32px",
    minWidth: "32px",
    margin: "0",
    padding: "0 11px",
    border: "1px solid var(--border-light)",
    borderRadius: "7px",
    backgroundColor: "color-mix(in oklch, var(--bg-primary) 92%, transparent)",
    backgroundImage: "none",
    boxShadow: "0 1px 0 color-mix(in oklch, var(--text-primary) 5%, transparent)",
    color: "var(--text-primary)",
    cursor: "pointer",
    fontFamily: "inherit",
    fontSize: "12px",
    fontWeight: "600",
    lineHeight: "1",
    transition:
      "border-color var(--motion-duration-quick) var(--ease-default), background-color var(--motion-duration-quick) var(--ease-default), color var(--motion-duration-quick) var(--ease-default)",
  },
  ".cm-panel.cm-search button:hover": {
    borderColor: "color-mix(in oklch, var(--accent) 46%, var(--border))",
    backgroundColor: "color-mix(in oklch, var(--accent-light) 50%, var(--bg-primary))",
    color: "var(--text-primary)",
  },
  ".cm-panel.cm-search button:active": {
    transform: "translateY(1px)",
  },
  ".cm-panel.cm-search button:focus-visible": {
    outline: "2px solid color-mix(in oklch, var(--accent) 42%, transparent)",
    outlineOffset: "2px",
  },
  ".cm-panel.cm-search button[name='next'], .cm-panel.cm-search button[name='replace']": {
    borderColor: "color-mix(in oklch, var(--accent) 52%, var(--border-light))",
    backgroundColor: "color-mix(in oklch, var(--accent-light) 42%, var(--bg-primary))",
    color: "var(--accent)",
  },
  ".cm-panel.cm-search button[name='next']": {
    gridColumn: "2",
    gridRow: "1",
  },
  ".cm-panel.cm-search button[name='prev']": {
    gridColumn: "3",
    gridRow: "1",
  },
  ".cm-panel.cm-search button[name='select']": {
    gridColumn: "4",
    gridRow: "1",
  },
  ".cm-panel.cm-search label:nth-of-type(1)": {
    gridColumn: "5",
    gridRow: "1",
  },
  ".cm-panel.cm-search label:nth-of-type(2)": {
    gridColumn: "6",
    gridRow: "1",
  },
  ".cm-panel.cm-search label:nth-of-type(3)": {
    gridColumn: "7",
    gridRow: "1",
  },
  ".cm-panel.cm-search button[name='replace']": {
    gridColumn: "2",
    gridRow: "2",
  },
  ".cm-panel.cm-search button[name='replaceAll']": {
    gridColumn: "3 / span 2",
    gridRow: "2",
    color: "var(--text-primary)",
  },
  ".cm-panel.cm-search button[name='close']": {
    gridColumn: "8",
    gridRow: "1",
    alignSelf: "center",
    width: "32px",
    padding: "0",
    borderColor: "transparent",
    backgroundColor: "transparent",
    boxShadow: "none",
    color: "var(--text-tertiary)",
    fontSize: "18px",
    fontWeight: "500",
  },
  ".cm-panel.cm-search button[name='close']:hover": {
    backgroundColor: "color-mix(in oklch, var(--danger-bg) 78%, transparent)",
    color: "var(--danger)",
  },
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in oklch, var(--warning) 32%, transparent)",
    outline: "1px solid color-mix(in oklch, var(--warning) 42%, transparent)",
  },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in oklch, var(--accent) 38%, transparent)",
    outlineColor: "var(--accent)",
  },
});
