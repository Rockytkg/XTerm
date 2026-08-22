export function createAsyncListenerRegistry() {
  const listeners = new Set();
  let disposed = false;

  function add(unlisten) {
    if (typeof unlisten !== "function") return null;
    if (disposed) {
      unlisten();
      return null;
    }
    listeners.add(unlisten);
    return unlisten;
  }

  function register(unlistenPromise) {
    if (typeof unlistenPromise === "function") {
      return Promise.resolve(add(unlistenPromise));
    }
    return Promise.resolve(unlistenPromise)
      .then((unlisten) => {
        return add(unlisten);
      })
      .catch(() => null);
  }

  // 监听器被提前单独取消时从集合移除，否则 dispose 会对同一 unlisten 重复调用
  function remove(unlisten) {
    listeners.delete(unlisten);
  }

  function dispose() {
    disposed = true;
    listeners.forEach((unlisten) => {
      unlisten?.();
    });
    listeners.clear();
  }

  return {
    add,
    dispose,
    remove,
    get disposed() {
      return disposed;
    },
    register,
  };
}
