import { onBeforeUnmount, ref } from "vue";
import {
  createSftpDragNavigator,
  isSameOrChildPath,
  normalizeRemotePath,
} from "./sftpDragNavigation";
import {
  NAME_CONFLICT_ACTION,
  renameRemoteEntry,
  resolveNameConflict,
} from "./sftpRemoteOperations";
import { createRafThrottle } from "../utils/schedulers";

const LEFT_MOUSE_BUTTON = 0;
const LONG_PRESS_MS = 180;
const GHOST_OFFSET = 12;

function safeMessage(error) {
  return String(error?.message || error || "");
}

export function useSftpMoveDrag({
  props,
  remotePath,
  remoteParent,
  remoteFiles,
  remoteFileByName,
  remoteFileMapForPath,
  refreshCurrentDirectoryIncremental = () => {},
  selectedNames,
  inlineEdit,
  loading,
  errorMessage,
  refreshRemote,
  closeContextMenu,
  requestRenameConflictAction = () => Promise.resolve("cancel"),
  t,
}) {
  const tableBodyRef = ref(null);
  const moveDragActive = ref(false);
  const moveDropTargetPath = ref("");

  let pendingPress = null;
  let activeDrag = null;
  let longPressTimer = 0;
  let ghostEl = null;
  let suppressClick = false;
  let disposed = false;

  const updateActiveDragFromPointer = createRafThrottle((x, y) => {
    if (!activeDrag) return;
    positionGhost(x, y);
    navigator.updateFromPoint(x, y, activeDrag, {
      sourcePath: activeDrag.entry.path,
    });
  });

  const navigator = createSftpDragNavigator({
    remotePath,
    remoteParent,
    refreshRemote,
    setDropTargetPath: (path) => {
      moveDropTargetPath.value = path;
    },
  });

  function entryByPath(path) {
    return remoteFiles.value.find((entry) => entry.path === path) || null;
  }

  function clearLongPressTimer() {
    if (!longPressTimer) return;
    window.clearTimeout(longPressTimer);
    longPressTimer = 0;
  }

  function removeGhost() {
    ghostEl?.remove();
    ghostEl = null;
  }

  function createGhost(entry, sourceRow, x, y) {
    removeGhost();
    const rect = sourceRow.getBoundingClientRect();
    const ghost = document.createElement("div");
    ghost.className = "sftp-move-drag-ghost";
    ghost.style.width = `${Math.max(180, Math.min(rect.width, 520))}px`;

    const kind = document.createElement("span");
    kind.className = "sftp-move-drag-ghost-kind";
    kind.textContent = entry.kind === "dir" ? t("sftp.folder") : t("sftp.file");

    const name = document.createElement("span");
    name.className = "sftp-move-drag-ghost-name";
    name.textContent = entry.name;

    ghost.append(kind, name);
    document.body.appendChild(ghost);
    ghostEl = ghost;
    positionGhost(x, y);
  }

  function positionGhost(x, y) {
    if (!ghostEl) return;
    ghostEl.style.transform = `translate3d(${x + GHOST_OFFSET}px, ${y + GHOST_OFFSET}px, 0)`;
  }

  function clearPendingPress() {
    clearLongPressTimer();
    pendingPress = null;
    window.removeEventListener("mousemove", onPendingMouseMove, true);
    window.removeEventListener("mouseup", onPendingMouseUp, true);
    window.removeEventListener("blur", cancelDrag, true);
  }

  function resetDragState() {
    clearPendingPress();
    navigator.reset();
    updateActiveDragFromPointer.cancel();
    removeGhost();
    activeDrag = null;
    moveDragActive.value = false;
    document.body.classList.remove("sftp-move-dragging");
    window.removeEventListener("mousemove", onActiveMouseMove, true);
    window.removeEventListener("mouseup", onActiveMouseUp, true);
    window.removeEventListener("blur", cancelDrag, true);
  }

  function sessionSnapshot() {
    return {
      connectionId: props.connection?.id || "",
      sessionId: props.sessionId || "",
    };
  }

  function isStaleSession(session) {
    return (
      disposed ||
      !session.connectionId ||
      !session.sessionId ||
      props.connection?.id !== session.connectionId ||
      props.sessionId !== session.sessionId
    );
  }

  function cancelDrag() {
    resetDragState();
  }

  async function commitMove(drag) {
    const session = sessionSnapshot();
    if (isStaleSession(session)) return;
    const destinationDirectory = drag.destinationDirectory || remotePath.value;
    const sourceDirectory = drag.sourceDirectory || remotePath.value;
    const fromPath = drag.entry.path;

    if (normalizeRemotePath(sourceDirectory) === normalizeRemotePath(destinationDirectory)) {
      return;
    }

    if (drag.entry.kind === "dir" && isSameOrChildPath(destinationDirectory, fromPath)) {
      if (isStaleSession(session)) return;
      errorMessage.value = t("sftp.moveIntoSelf");
      await refreshRemote(remotePath.value, { suppressError: true });
      return;
    }

    try {
      const targetEntries =
        normalizeRemotePath(destinationDirectory) === normalizeRemotePath(remotePath.value)
          ? remoteFileByName.value || new Map()
          : await remoteFileMapForPath(destinationDirectory);
      if (isStaleSession(session)) return;
      const conflict = await resolveNameConflict({
        sourcePath: drag.entry.path,
        sourceEntry: drag.entry,
        targetFileByName: targetEntries,
        targetName: drag.entry.name,
        requestConflictAction: requestRenameConflictAction,
        defaultAction: NAME_CONFLICT_ACTION.CREATE,
        skipAction: NAME_CONFLICT_ACTION.CANCEL,
      });
      if (conflict.cancelled) {
        return;
      }
      if (isStaleSession(session)) return;
      await renameRemoteEntry({
        connectionId: session.connectionId,
        sessionId: session.sessionId,
        fromPath,
        toParentPath: destinationDirectory,
        toName: drag.entry.name,
        conflictAction: conflict.action,
      });
      if (isStaleSession(session)) return;
      await refreshCurrentDirectoryIncremental();
    } catch (error) {
      if (isStaleSession(session)) return;
      errorMessage.value = `${t("sftp.moveFailed")}: ${safeMessage(error)}`;
      await refreshRemote(remotePath.value, { suppressError: true });
    }
  }

  async function finishDrag() {
    if (!activeDrag) {
      resetDragState();
      return;
    }
    const drag = activeDrag;
    drag.active = false;
    resetDragState();
    if (!drag.dropAllowed) return;
    await commitMove(drag);
  }

  function beginDrag() {
    if (!pendingPress || !props.connection || !props.sessionId) return;
    const { entry, row, x, y } = pendingPress;
    clearPendingPress();

    activeDrag = {
      active: true,
      entry,
      sourceDirectory: remotePath.value,
      destinationDirectory: remotePath.value,
      dropAllowed: true,
    };
    selectedNames.value = new Set([entry.name]);
    closeContextMenu();
    moveDragActive.value = true;
    document.body.classList.add("sftp-move-dragging");
    createGhost(entry, row, x, y);
    suppressClick = true;

    window.addEventListener("mousemove", onActiveMouseMove, true);
    window.addEventListener("mouseup", onActiveMouseUp, true);
    window.addEventListener("blur", cancelDrag, true);
  }

  function onPendingMouseMove(event) {
    if (!pendingPress) {
      clearPendingPress();
      return;
    }
    event.preventDefault();
    pendingPress.x = event.clientX;
    pendingPress.y = event.clientY;
  }

  function onPendingMouseUp(event) {
    if (event.button !== LEFT_MOUSE_BUTTON) return;
    clearPendingPress();
  }

  function onActiveMouseMove(event) {
    if (!activeDrag) {
      finishDrag();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    updateActiveDragFromPointer(event.clientX, event.clientY);
  }

  function onActiveMouseUp(event) {
    if (event.button !== LEFT_MOUSE_BUTTON) return;
    event.preventDefault();
    event.stopPropagation();
    updateActiveDragFromPointer.flush();
    finishDrag();
  }

  function onMoveMouseDown(event) {
    if (
      event.button !== LEFT_MOUSE_BUTTON ||
      !props.visible ||
      loading.value ||
      inlineEdit.value.active
    )
      return;
    const row = event.target?.closest?.(".sftp-move-draggable");
    if (!row || !tableBodyRef.value?.contains(row)) return;

    const entry = entryByPath(row.dataset.path || "");
    if (!entry || entry.name === "..") return;

    event.preventDefault();
    clearPendingPress();
    pendingPress = {
      entry: { ...entry },
      row,
      x: event.clientX,
      y: event.clientY,
    };
    longPressTimer = window.setTimeout(beginDrag, LONG_PRESS_MS);
    window.addEventListener("mousemove", onPendingMouseMove, true);
    window.addEventListener("mouseup", onPendingMouseUp, true);
    window.addEventListener("blur", cancelDrag, true);
  }

  function suppressMoveClick(event) {
    if (!suppressClick) return;
    suppressClick = false;
    event.preventDefault();
    event.stopPropagation();
  }

  onBeforeUnmount(() => {
    disposed = true;
    resetDragState();
  });

  return {
    moveDragActive,
    moveDropTargetPath,
    onMoveMouseDown,
    suppressMoveClick,
    tableBodyRef,
  };
}
