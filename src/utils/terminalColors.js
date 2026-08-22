const ANSI_NAMES = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
];

const XTERM_256 = buildXterm256Palette();

function buildXterm256Palette() {
  const colors = [];
  const cube = [0, 95, 135, 175, 215, 255];
  for (const r of cube) {
    for (const g of cube) {
      for (const b of cube) {
        colors.push(hex(r, g, b));
      }
    }
  }
  for (let i = 0; i < 24; i += 1) {
    const value = 8 + i * 10;
    colors.push(hex(value, value, value));
  }
  return colors;
}

function hex(r, g, b) {
  return `#${[r, g, b].map((value) => value.toString(16).padStart(2, "0")).join("")}`;
}

function terminalTheme({
  background,
  foreground,
  cursor,
  cursorAccent,
  selectionBackground,
  selectionInactiveBackground,
  ansi,
}) {
  return {
    background,
    foreground,
    cursor,
    cursorAccent,
    selectionBackground,
    selectionInactiveBackground: selectionInactiveBackground ?? selectionBackground,
    extendedAnsi: XTERM_256,
    ...Object.fromEntries(ANSI_NAMES.map((name, index) => [name, ansi[index]])),
  };
}

// Every palette tracks its canonical upstream spec (VS Code theme JSON,
// Solarized/Nord/Gruvbox/Catppuccin official ANSI mappings, GitHub Primer,
// One Half Dark, ...). Inactive selection is a dimmer variant of the active
// one so a defocused pane keeps its selection visible but recessed.
const TERMINAL_THEMES = {
  // VS Code Dark+ integrated terminal.
  default: terminalTheme({
    background: "#1e1e1e",
    foreground: "#cccccc",
    cursor: "#cccccc",
    cursorAccent: "#1e1e1e",
    selectionBackground: "rgba(38, 79, 120, 0.72)",
    selectionInactiveBackground: "rgba(38, 79, 120, 0.45)",
    ansi: [
      "#000000",
      "#cd3131",
      "#0dbc79",
      "#e5e510",
      "#2472c8",
      "#bc3fbc",
      "#11a8cd",
      "#e5e5e5",
      "#666666",
      "#f14c4c",
      "#23d18b",
      "#f5f543",
      "#3b8eea",
      "#d670d6",
      "#29b8db",
      "#ffffff",
    ],
  }),
  // VS Code Light+ integrated terminal.
  light: terminalTheme({
    background: "#ffffff",
    foreground: "#333333",
    cursor: "#333333",
    cursorAccent: "#ffffff",
    selectionBackground: "rgba(0, 120, 215, 0.32)",
    selectionInactiveBackground: "rgba(0, 120, 215, 0.2)",
    ansi: [
      "#000000",
      "#cd3131",
      "#00bc00",
      "#949800",
      "#0451a5",
      "#bc05bc",
      "#0598bc",
      "#555555",
      "#666666",
      "#cd3131",
      "#14ce14",
      "#b5ba00",
      "#0451a5",
      "#bc05bc",
      "#0598bc",
      "#a5a5a5",
    ],
  }),
  // Official Dracula spec (selection = current line #44475a).
  dracula: terminalTheme({
    background: "#282a36",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    cursorAccent: "#282a36",
    selectionBackground: "rgba(68, 71, 90, 0.92)",
    selectionInactiveBackground: "rgba(68, 71, 90, 0.55)",
    ansi: [
      "#21222c",
      "#ff5555",
      "#50fa7b",
      "#f1fa8c",
      "#bd93f9",
      "#ff79c6",
      "#8be9fd",
      "#f8f8f2",
      "#6272a4",
      "#ff6e6e",
      "#69ff94",
      "#ffffa5",
      "#d6acff",
      "#ff92df",
      "#a4ffff",
      "#ffffff",
    ],
  }),
  // VS Code Monokai terminal (yellow #f4bf75 pairs with bright yellow).
  monokai: terminalTheme({
    background: "#272822",
    foreground: "#f8f8f2",
    cursor: "#f8f8f0",
    cursorAccent: "#272822",
    selectionBackground: "rgba(73, 72, 62, 0.92)",
    selectionInactiveBackground: "rgba(73, 72, 62, 0.55)",
    ansi: [
      "#272822",
      "#f92672",
      "#a6e22e",
      "#f4bf75",
      "#66d9ef",
      "#ae81ff",
      "#a1efe4",
      "#f8f8f2",
      "#75715e",
      "#f92672",
      "#a6e22e",
      "#f4bf75",
      "#66d9ef",
      "#ae81ff",
      "#a1efe4",
      "#f9f8f5",
    ],
  }),
  // Official Solarized Dark terminal mapping; cursor uses base1 so the block
  // cursor stays distinguishable from base0 body text.
  solarized: terminalTheme({
    background: "#002b36",
    foreground: "#839496",
    cursor: "#93a1a1",
    cursorAccent: "#002b36",
    selectionBackground: "rgba(7, 54, 66, 0.92)",
    selectionInactiveBackground: "rgba(7, 54, 66, 0.55)",
    ansi: [
      "#073642",
      "#dc322f",
      "#859900",
      "#b58900",
      "#268bd2",
      "#d33682",
      "#2aa198",
      "#eee8d5",
      "#002b36",
      "#cb4b16",
      "#586e75",
      "#657b83",
      "#839496",
      "#6c71c4",
      "#93a1a1",
      "#fdf6e3",
    ],
  }),
  // SecureCRT 8.3+ 默认配色即此方案：Solarized Light 前景/背景 + 官方 Solarized ANSI 映射
  // （前景 base00 65 7b 83、背景 base3 fd f6 e3，见 VanDyke colorconfig 文档）
  solarizedLight: terminalTheme({
    background: "#fdf6e3",
    foreground: "#657b83",
    cursor: "#586e75",
    cursorAccent: "#fdf6e3",
    selectionBackground: "rgba(238, 232, 213, 0.92)",
    selectionInactiveBackground: "rgba(238, 232, 213, 0.6)",
    ansi: [
      "#073642",
      "#dc322f",
      "#859900",
      "#b58900",
      "#268bd2",
      "#d33682",
      "#2aa198",
      "#eee8d5",
      "#002b36",
      "#cb4b16",
      "#586e75",
      "#657b83",
      "#839496",
      "#6c71c4",
      "#93a1a1",
      "#fdf6e3",
    ],
  }),
  // Official Tango palette.
  tango: terminalTheme({
    background: "#2e3436",
    foreground: "#d3d7cf",
    cursor: "#d3d7cf",
    cursorAccent: "#2e3436",
    selectionBackground: "rgba(85, 87, 83, 0.9)",
    selectionInactiveBackground: "rgba(85, 87, 83, 0.55)",
    ansi: [
      "#2e3436",
      "#cc0000",
      "#4e9a06",
      "#c4a000",
      "#3465a4",
      "#75507b",
      "#06989a",
      "#d3d7cf",
      "#555753",
      "#ef2929",
      "#8ae234",
      "#fce94f",
      "#729fcf",
      "#ad7fa8",
      "#34e2e2",
      "#eeeeec",
    ],
  }),
  // One Half Dark terminal: bright variants must be at least as bright as
  // their normal counterparts (Atom one-dark ships #be5046/#d19a66 as
  // secondary hues, which read as *dimmer* brights — avoided here).
  oneDark: terminalTheme({
    background: "#282c34",
    foreground: "#abb2bf",
    cursor: "#528bff",
    cursorAccent: "#282c34",
    selectionBackground: "rgba(62, 68, 81, 0.92)",
    selectionInactiveBackground: "rgba(62, 68, 81, 0.55)",
    ansi: [
      "#3f4451",
      "#e06c75",
      "#98c379",
      "#e5c07b",
      "#61afef",
      "#c678dd",
      "#56b6c2",
      "#dcdfe4",
      "#5c6370",
      "#e06c75",
      "#98c379",
      "#e5c07b",
      "#61afef",
      "#c678dd",
      "#56b6c2",
      "#ffffff",
    ],
  }),
  // Official Nord terminal (selection = nord2 #434c5e).
  nord: terminalTheme({
    background: "#2e3440",
    foreground: "#d8dee9",
    cursor: "#d8dee9",
    cursorAccent: "#2e3440",
    selectionBackground: "rgba(67, 76, 94, 0.92)",
    selectionInactiveBackground: "rgba(67, 76, 94, 0.55)",
    ansi: [
      "#3b4252",
      "#bf616a",
      "#a3be8c",
      "#ebcb8b",
      "#81a1c1",
      "#b48ead",
      "#88c0d0",
      "#e5e9f0",
      "#4c566a",
      "#bf616a",
      "#a3be8c",
      "#ebcb8b",
      "#81a1c1",
      "#b48ead",
      "#8fbcbb",
      "#eceff4",
    ],
  }),
  // Official Gruvbox Dark terminal (selection = bg3 #665c54).
  gruvbox: terminalTheme({
    background: "#282828",
    foreground: "#ebdbb2",
    cursor: "#ebdbb2",
    cursorAccent: "#282828",
    selectionBackground: "rgba(102, 92, 84, 0.9)",
    selectionInactiveBackground: "rgba(102, 92, 84, 0.55)",
    ansi: [
      "#282828",
      "#cc241d",
      "#98971a",
      "#d79921",
      "#458588",
      "#b16286",
      "#689d6a",
      "#a89984",
      "#928374",
      "#fb4934",
      "#b8bb26",
      "#fabd2f",
      "#83a598",
      "#d3869b",
      "#8ec07c",
      "#ebdbb2",
    ],
  }),
  // Official Tokyo Night terminal.
  tokyoNight: terminalTheme({
    background: "#1a1b26",
    foreground: "#c0caf5",
    cursor: "#c0caf5",
    cursorAccent: "#1a1b26",
    selectionBackground: "rgba(51, 70, 124, 0.9)",
    selectionInactiveBackground: "rgba(51, 70, 124, 0.55)",
    ansi: [
      "#15161e",
      "#f7768e",
      "#9ece6a",
      "#e0af68",
      "#7aa2f7",
      "#bb9af7",
      "#7dcfff",
      "#a9b1d6",
      "#414868",
      "#f7768e",
      "#9ece6a",
      "#e0af68",
      "#7aa2f7",
      "#bb9af7",
      "#7dcfff",
      "#c0caf5",
    ],
  }),
  // Official Catppuccin Mocha terminal (selection = surface2 #585b70).
  catppuccin: terminalTheme({
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#f5e0dc",
    cursorAccent: "#1e1e2e",
    selectionBackground: "rgba(88, 91, 112, 0.8)",
    selectionInactiveBackground: "rgba(88, 91, 112, 0.5)",
    ansi: [
      "#45475a",
      "#f38ba8",
      "#a6e3a1",
      "#f9e2af",
      "#89b4fa",
      "#f5c2e7",
      "#94e2d5",
      "#bac2de",
      "#585b70",
      "#f38ba8",
      "#a6e3a1",
      "#f9e2af",
      "#89b4fa",
      "#f5c2e7",
      "#94e2d5",
      "#a6adc8",
    ],
  }),
  // GitHub Dark Default (github-vscode-theme) terminal.
  githubDark: terminalTheme({
    background: "#0d1117",
    foreground: "#c9d1d9",
    cursor: "#c9d1d9",
    cursorAccent: "#0d1117",
    selectionBackground: "rgba(38, 79, 120, 0.9)",
    selectionInactiveBackground: "rgba(38, 79, 120, 0.55)",
    ansi: [
      "#484f58",
      "#ff7b72",
      "#3fb950",
      "#d29922",
      "#58a6ff",
      "#bc8cff",
      "#39c5cf",
      "#b1bac4",
      "#6e7681",
      "#ffa198",
      "#56d364",
      "#e3b341",
      "#79c0ff",
      "#d2a8ff",
      "#56d4dd",
      "#f0f6fc",
    ],
  }),
  // GitHub Light Default (github-vscode-theme) terminal.
  githubLight: terminalTheme({
    background: "#ffffff",
    foreground: "#24292f",
    cursor: "#24292f",
    cursorAccent: "#ffffff",
    selectionBackground: "rgba(9, 105, 218, 0.25)",
    selectionInactiveBackground: "rgba(9, 105, 218, 0.16)",
    ansi: [
      "#24292f",
      "#cf222e",
      "#116329",
      "#4d2d00",
      "#0969da",
      "#8250df",
      "#1b7c83",
      "#6e7781",
      "#57606a",
      "#a40e26",
      "#1a7f37",
      "#633c01",
      "#218bff",
      "#a475f9",
      "#3192aa",
      "#8c959f",
    ],
  }),
};

export const TERMINAL_THEME_NAMES = Object.keys(TERMINAL_THEMES);

export function getTerminalTheme(name) {
  return TERMINAL_THEMES[name] ?? TERMINAL_THEMES.default;
}

export function getTerminalStatusPalette(name) {
  const theme = getTerminalTheme(name);
  // Solarized maps brightBlack to base03 — identical to its background — so
  // it cannot serve as a visible hint tone; base00 (brightYellow slot) is
  // the official muted-content color and stays readable.
  const hint =
    theme.brightBlack.toLowerCase() === theme.background.toLowerCase()
      ? theme.brightYellow
      : theme.brightBlack;
  return {
    boot: theme.brightBlue,
    hint,
    success: theme.green,
    error: theme.red,
    info: theme.blue,
  };
}
