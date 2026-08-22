/**
 * 通用轻量事件总线。
 *
 * 与 Tauri 的 event bridge 解耦：仅负责同一线程内的订阅/派发，
 * 不处理跨 WebView/Rust 边界的事件传输。
 */
export function createEventBus({ logger } = {}) {
  /** @type {Map<string, Set<Function>>} */
  const listeners = new Map();

  function on(type, handler) {
    const set = listeners.get(type) ?? new Set();
    set.add(handler);
    listeners.set(type, set);
    return () => {
      off(type, handler);
    };
  }

  function off(type, handler) {
    const set = listeners.get(type);
    if (!set) return;
    set.delete(handler);
    if (set.size === 0) listeners.delete(type);
  }

  function emit(type, payload) {
    const set = listeners.get(type);
    const listenerCount = set?.size ?? 0;
    if (!listenerCount) return 0;
    for (const handler of [...set]) {
      try {
        handler(payload);
      } catch (error) {
        logger?.error("eventBus listener failed:", type, error);
      }
    }
    return listenerCount;
  }

  function clear() {
    listeners.clear();
  }

  return { clear, emit, off, on };
}
