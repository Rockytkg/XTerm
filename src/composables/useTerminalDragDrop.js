import { getCurrentWindow } from "@tauri-apps/api/window";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";

export function useTerminalDragDrop({ logger, shouldListen, handleDrop }) {
  let unlistenDragDrop = null;
  // register 是异步的，onMounted/onActivated 可能连续触发 attach；
  // 用 in-flight Promise 去重，避免同一面板注册多个窗口级监听
  let attachPromise = null;
  const asyncListeners = createAsyncListenerRegistry();

  function attachDragDropListener() {
    if (unlistenDragDrop || !shouldListen()) return Promise.resolve();
    if (attachPromise) return attachPromise;
    let registration;
    try {
      registration = asyncListeners.register(
        getCurrentWindow().onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "drop" && payload.paths?.length) {
            handleDrop(payload.paths);
          }
        }),
      );
    } catch (error) {
      logger.warn("Failed to subscribe terminal drag/drop events:", error);
      return Promise.resolve();
    }
    // register 失败（含已 dispose）时 resolve 为 null，unlistenDragDrop 保持 null，下次 attach 可重试
    attachPromise = registration
      .then((unlisten) => {
        unlistenDragDrop = unlisten;
      })
      .finally(() => {
        attachPromise = null;
      });
    return attachPromise;
  }

  function detachDragDropListener() {
    // attach 仍在途时等其 settle 后再取消，否则新注册的监听会残留
    if (attachPromise) {
      void attachPromise.then(() => {
        releaseDragDropListener();
      });
      return;
    }
    releaseDragDropListener();
  }

  function releaseDragDropListener() {
    const unlisten = unlistenDragDrop;
    unlistenDragDrop = null;
    if (!unlisten) return;
    unlisten();
    // 单独取消后同步移出注册表，避免重新 attach 后 dispose 重复调用旧的 unlisten
    asyncListeners.remove(unlisten);
  }

  function disposeDragDropListener() {
    detachDragDropListener();
    asyncListeners.dispose();
  }

  return {
    attachDragDropListener,
    detachDragDropListener,
    disposeDragDropListener,
  };
}
