import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createLogger } from "../utils/logger";
import { createSftpDragNavigator } from "./sftpDragNavigation";
import { createAsyncListenerLifecycle } from "../utils/asyncListeners";
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

  let uploadSession = null;
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

  const dragDropListener = createAsyncListenerLifecycle({
    canAttach: () => props.visible,
    subscribe: () =>
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
    onSubscribeError: (error) => logger.error("sftp.drag-drop.subscribe.failed", error),
  });

  function attachDragDropListener() {
    return dragDropListener.attach();
  }

  function detachDragDropListener() {
    resetUploadDrag();
    dragDropListener.detach();
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
    resetUploadDrag();
    dragDropListener.dispose();
  });

  return {
    dragActive,
    dropTargetPath,
    onTableDomDragLeave,
    onTableDomDragOver,
    onTableDomDrop,
  };
}
