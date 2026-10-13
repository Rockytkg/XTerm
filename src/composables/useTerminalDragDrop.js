import { getCurrentWindow } from "@tauri-apps/api/window";
import { createAsyncListenerLifecycle } from "../utils/asyncListeners";

export function useTerminalDragDrop({ logger, shouldListen, handleDrop }) {
  const dragDropListener = createAsyncListenerLifecycle({
    canAttach: shouldListen,
    subscribe: () =>
      getCurrentWindow().onDragDropEvent((event) => {
        const payload = event.payload;
        // attach 后开关可能被关掉，drop 时复查一次，避免禁用后仍触发上传
        if (payload.type === "drop" && payload.paths?.length && shouldListen()) {
          handleDrop(payload.paths);
        }
      }),
    onSubscribeError: (error) =>
      logger.warn("Failed to subscribe terminal drag/drop events:", error),
  });

  return {
    attachDragDropListener: dragDropListener.attach,
    detachDragDropListener: dragDropListener.detach,
    disposeDragDropListener: dragDropListener.dispose,
  };
}
