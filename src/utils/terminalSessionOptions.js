export const DEFAULT_TERMINAL_TYPE = "xterm-256color";
export const DEFAULT_TERMINAL_ENCODING = "auto";

export const BACKSPACE_SENDS = Object.freeze({
  DEL: "DEL",
  BS: "BS",
});

export const TERMINAL_TYPE_OPTIONS = Object.freeze([
  { label: "xterm-256color", value: "xterm-256color" },
  { label: "xterm", value: "xterm" },
  { label: "xterm-color", value: "xterm-color" },
  { label: "vt220", value: "vt220" },
  { label: "vt100", value: "vt100" },
  { label: "ansi", value: "ansi" },
  { label: "linux", value: "linux" },
]);

export const BACKSPACE_SENDS_OPTIONS = Object.freeze([
  { label: "DEL (^?)", value: BACKSPACE_SENDS.DEL },
  { label: "BS (^H)", value: BACKSPACE_SENDS.BS },
]);

const ENCODING_OPTION_VALUES = Object.freeze(["UTF-8", "GB18030", "BIG5", "SHIFT_JIS", "EUC-KR"]);

export function createEncodingOptions(t) {
  return [
    { label: t("connectionDialog.fields.encodingAuto"), value: DEFAULT_TERMINAL_ENCODING },
    ...ENCODING_OPTION_VALUES.map((value) => ({ label: value, value })),
  ];
}
