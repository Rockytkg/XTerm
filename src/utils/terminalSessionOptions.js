export const DEFAULT_TERMINAL_TYPE = "xterm-256color";
export const DEFAULT_TERMINAL_ENCODING = "auto";

export const BACKSPACE_SENDS = Object.freeze({
  DEL: "DEL",
  BS: "BS",
});

// 退格键默认发送 BS(^H)：VT100/网络设备 CLI 惯例，主流行编辑（readline 等）
// 同样接受；只有明确只认 DEL(^?) 的对端才需要切换。
export const BACKSPACE_SENDS_OPTIONS = Object.freeze([
  { label: "BS (^H)", value: BACKSPACE_SENDS.BS },
  { label: "DEL (^?)", value: BACKSPACE_SENDS.DEL },
]);

export const TERMINAL_TYPE_OPTIONS = Object.freeze([
  { label: "xterm-256color", value: "xterm-256color" },
  { label: "xterm", value: "xterm" },
  { label: "xterm-color", value: "xterm-color" },
  { label: "vt220", value: "vt220" },
  { label: "vt100", value: "vt100" },
  { label: "ansi", value: "ansi" },
  { label: "linux", value: "linux" },
]);

const ENCODING_OPTION_VALUES = Object.freeze(["UTF-8", "GB18030", "BIG5", "SHIFT_JIS", "EUC-KR"]);

export function createEncodingOptions(t) {
  return [
    { label: t("connectionDialog.fields.encodingAuto"), value: DEFAULT_TERMINAL_ENCODING },
    ...ENCODING_OPTION_VALUES.map((value) => ({ label: value, value })),
  ];
}
