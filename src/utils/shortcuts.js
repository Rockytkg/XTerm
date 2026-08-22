const MODIFIER_KEYS = new Set(["ctrl", "alt", "shift", "meta"]);

const KEY_ALIASES = {
  " ": "space",
  cmd: "meta",
  command: "meta",
  control: "ctrl",
  option: "alt",
};

function normalizeShortcutKey(key) {
  const normalized = String(key || "")
    .trim()
    .toLowerCase();
  return KEY_ALIASES[normalized] ?? normalized;
}

function normalizeShortcut(value) {
  return String(value || "")
    .split("+")
    .map((part) => normalizeShortcutKey(part))
    .filter(Boolean)
    .sort()
    .join("+");
}

function eventShortcut(event) {
  const parts = [];
  if (event.ctrlKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  if (event.metaKey) parts.push("meta");

  const key =
    event.code === "Space" || event.key === " " ? "space" : normalizeShortcutKey(event.key);
  if (!MODIFIER_KEYS.has(key)) {
    parts.push(key);
  }

  return parts.sort().join("+");
}

export function shortcutMatchesEvent(shortcut, event) {
  const normalized = normalizeShortcut(shortcut);
  return normalized === eventShortcut(event);
}
