import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createLogger } from "../utils/logger";
import { createSftpDragNavigator } from "./sftpDragNavigation";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { createRafThrottle } from "../utils/schedulers";

const logger = createLogger("frontend.sftp.drag_drop");

function pathsFromDrop(event, onUnavailablePath) {
  const files = Array.from(event.dataTransfer?.files ?? []);
  const paths = files.map((file) => file.path).filter(Boolean);
  if (!paths.length && files.length) {
    onUnavailablePath();
  }
  return paths;
}

function cssPointFromPhysical(position) {
  return {
    x: Number(position?.x || 0) / (window.devicePixelRatio || 1),
    y: Number(position?.y || 0) / (window.devicePixelRatio || 1),
  };
}

export function useSftpDragDrop({
  props,
  remotePath,
  remoteParent,
  errorMessage,
  refreshRemote,
  uploadLocalPaths,
  t,
}) {
  const dragActive = ref(false);
  const dropTargetPath = ref("");

  let unlistenDragDrop = null;
  // register 是异步的，attach 可能连续触发；用 in-flight Promise 去重，避免重复注册窗口级监听
  let attachPromise = null;
  let uploadSession = null;
  const asyncListeners = createAsyncListenerRegistry();
  const scheduleUploadTargetUpdate = createRafThrottle(updateUploadTargetAt);

  const navigator = createSftpDragNavigator({
    remotePath,
    remoteParent,
    refreshRemote,
    setDropTargetPath: (path) => {
      dropTargetPath.value = path;
    },
  });

  function ensureUploadSession() {
    if (!uploadSession) {
      uploadSession = {
        active: true,
        destinationDirectory: remotePath.value,
        dropAllowed: false,
      };
    }
    return uploadSession;
  }

  function updateUploadTargetAt(x, y) {
    const session = ensureUploadSession();
    const target = navigator.updateFromPoint(x, y, session);
    dragActive.value = target.insideBrowser;
    return session.dropAllowed ? session.destinationDirectory : "";
  }

  function resetUploadDrag() {
    scheduleUploadTargetUpdate.cancel();
    if (uploadSession) uploadSession.active = false;
    uploadSession = null;
    dragActive.value = false;
    navigator.reset();
  }

  function uploadToCurrentTarget(paths, x, y) {
    scheduleUploadTargetUpdate.cancel();
    if (!paths.length) {
      resetUploadDrag();
      return;
    }
    const targetDirectory = updateUploadTargetAt(x, y);
    resetUploadDrag();
    if (targetDirectory) {
      uploadLocalPaths(paths, targetDirectory);
    }
  }

  function onTableDomDragOver(event) {
    event.preventDefault();
    scheduleUploadTargetUpdate(event.clientX, event.clientY);
  }

  function onTableDomDragLeave(event) {
    const relatedTarget = event.relatedTarget;
    if (relatedTarget instanceof Node && event.currentTarget?.contains?.(relatedTarget)) return;
    resetUploadDrag();
  }

  function onTableDomDrop(event) {
    event.preventDefault();
    uploadToCurrentTarget(
      pathsFromDrop(event, () => {
        errorMessage.value = t("sftp.dropPathUnavailable");
      }),
      event.clientX,
      event.clientY,
    );
  }

  function attachDragDropListener() {
    if (unlistenDragDrop || !props.visible) return Promise.resolve();
    if (attachPromise) return attachPromise;
    let registration;
    try {
      registration = asyncListeners.register(
        getCurrentWindow().onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "enter" || payload.type === "over") {
            const point = cssPointFromPhysical(payload.position);
            scheduleUploadTargetUpdate(point.x, point.y);
          } else if (payload.type === "drop") {
            const point = cssPointFromPhysical(payload.position);
            uploadToCurrentTarget(payload.paths || [], point.x, point.y);
          } else {
            resetUploadDrag();
          }
        }),
      );
    } catch (error) {
      logger.error("sftp.drag-drop.subscribe.failed", error);
      return Promise.resolve();
    }
    // register 失败（含已 dispose）时 resolve 为 null，unlistenDragDrop 保持空值，下次 attach 可重试
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
    resetUploadDrag();
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
    unlistenDragDrop = undefined;
    if (!unlisten) return;
    unlisten();
    // 单独取消后同步移出注册表，避免重新 attach 后 dispose 重复调用旧的 unlisten
    asyncListeners.remove(unlisten);
  }

  onMounted(async () => {
    await attachDragDropListener();
  });

  watch(
    () => props.visible,
    (visible) => {
      if (visible) {
        attachDragDropListener();
      } else {
        detachDragDropListener();
      }
    },
  );

  onBeforeUnmount(() => {
    detachDragDropListener();
    asyncListeners.dispose();
  });

  return {
    dragActive,
    dropTargetPath,
    onTableDomDragLeave,
    onTableDomDragOver,
    onTableDomDrop,
  };
}
