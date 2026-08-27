import { eventShortcut, normalizeShortcut } from "./shortcuts.js";

/**
 * 统一快捷键注册表：一个 target 只挂一个 keydown 监听，按注册顺序分发。
 *
 * - 每条注册包含 id、shortcut（字符串或返回字符串的函数）、可选 context
 *   与 when 条件；context 默认 "global"，未启用的 context 不参与匹配。
 * - handleEvent 返回 false 表示事件已被消费（调用方如 xterm 的
 *   attachCustomKeyEventHandler 借此阻止默认处理），true 表示继续传递。
 * - run 返回 "continue" 可让出本次匹配，继续尝试后续注册项。
 */
export function createShortcutRegistry({ target = null } = {}) {
  const entries = new Map();
  const enabledContexts = new Set(["global"]);
  let listening = false;

  function handleEvent(event) {
    if (event?.type !== "keydown") return true;
    const combo = eventShortcut(event);
    for (const entry of entries.values()) {
      if (!enabledContexts.has(entry.context)) continue;
      const shortcut = typeof entry.shortcut === "function" ? entry.shortcut() : entry.shortcut;
      if (!shortcut) continue;
      // 字符串快捷键用 register 时缓存的归一化结果；函数 shortcut 按事件动态求值。
      const normalized =
        typeof entry.shortcut === "function" ? normalizeShortcut(shortcut) : entry.normalized;
      if (normalized !== combo) continue;
      if (entry.when && !entry.when(event)) continue;
      if (entry.preventDefault !== false) event.preventDefault();
      const result = entry.run(event);
      if (result === "continue") continue;
      if (entry.stopPropagation) event.stopPropagation();
      return entry.consume === false;
    }
    return true;
  }

  const listener = (event) => {
    handleEvent(event);
  };

  function register({ id, shortcut, run, context = "global", when, ...options }) {
    if (!id || typeof run !== "function") {
      throw new Error("Shortcut registration requires an id and a run handler.");
    }
    // 静态快捷键的归一化结果在注册时缓存，避免每个 keydown 对每条注册项重算。
    const normalized = typeof shortcut === "function" ? null : normalizeShortcut(shortcut);
    entries.set(id, { id, shortcut, normalized, run, context, when, ...options });
    return () => entries.delete(id);
  }

  function unregister(id) {
    return entries.delete(id);
  }

  function enableContext(context) {
    enabledContexts.add(context);
  }

  function disableContext(context) {
    enabledContexts.delete(context);
  }

  function attach(nextTarget = target) {
    if (listening || !nextTarget) return;
    target = nextTarget;
    target.addEventListener("keydown", listener);
    listening = true;
  }

  function detach() {
    if (!listening || !target) return;
    target.removeEventListener("keydown", listener);
    listening = false;
  }

  function dispose() {
    detach();
    entries.clear();
    enabledContexts.clear();
    enabledContexts.add("global");
  }

  return {
    attach,
    detach,
    disableContext,
    dispose,
    enableContext,
    handleEvent,
    register,
    unregister,
  };
}
