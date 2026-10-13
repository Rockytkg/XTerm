import { createLogger } from "./logger";

const logger = createLogger("frontend.utils.async-listeners");

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
      .catch((error) => {
        logger.warn("listener.register.failed", error);
        return null;
      });
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

// 单个异步监听（如 getCurrentWindow().onDragDropEvent）的挂载生命周期：
// register 是异步的，attach 可能连续触发，用 in-flight Promise 去重避免重复注册；
// detach 在 attach 在途时等其 settle 后再取消，否则新注册的监听会残留。
// 调用方只提供订阅动作与挂载条件，日志文案经 onSubscribeError 保留在调用方。
export function createAsyncListenerLifecycle({
  canAttach = () => true,
  subscribe,
  onSubscribeError,
}) {
  const registry = createAsyncListenerRegistry();
  let unlisten = null;
  let attachPromise = null;

  function attach() {
    if (unlisten || !canAttach()) return Promise.resolve();
    if (attachPromise) return attachPromise;
    let registration;
    try {
      registration = registry.register(subscribe());
    } catch (error) {
      onSubscribeError?.(error);
      return Promise.resolve();
    }
    // register 失败（含已 dispose）时 resolve 为 null，unlisten 保持 null，下次 attach 可重试
    attachPromise = registration
      .then((nextUnlisten) => {
        unlisten = nextUnlisten;
      })
      .finally(() => {
        attachPromise = null;
      });
    return attachPromise;
  }

  function detach() {
    if (attachPromise) {
      void attachPromise.then(() => {
        release();
      });
      return;
    }
    release();
  }

  function release() {
    const current = unlisten;
    unlisten = null;
    if (!current) return;
    current();
    // 单独取消后同步移出注册表，避免重新 attach 后 dispose 重复调用旧的 unlisten
    registry.remove(current);
  }

  function dispose() {
    detach();
    registry.dispose();
  }

  return {
    attach,
    detach,
    dispose,
  };
}
