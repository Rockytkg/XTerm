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

export function normalizeShortcut(value) {
  const parts = String(value || "").split("+");
  // 主键为 "+" 的组合（如 "Ctrl++"）split 后会产生空尾段，还原为 "+" 键。
  if (parts.length > 1 && parts[parts.length - 1] === "") {
    parts.pop();
    parts[parts.length - 1] = "+";
  }
  return parts
    .map((part) => normalizeShortcutKey(part))
    .filter(Boolean)
    .sort()
    .join("+");
}

export function eventShortcut(event) {
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
