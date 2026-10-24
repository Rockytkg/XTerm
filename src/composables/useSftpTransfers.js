import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  cancelSftpTransfer,
  chooseSftpDownloadPath,
  chooseSftpUploadFiles,
  listSftpTransfers,
  pauseSftpTransfer,
  resumeSftpTransfer,
  transferSftp,
} from "../services/sftp";
import { TERMINAL_EVENTS, observeTerminalEvent } from "../events/terminalEventBus";
import {
  loadRemoteEntriesByName,
  NAME_CONFLICT_ACTION,
  resolveNameConflict,
} from "./sftpRemoteOperations";
import { createSftpSessionScope } from "./sftpSessionScope";
import { createLogger } from "../utils/logger";
import { createDebounced, createRafThrottle } from "../utils/schedulers";
import { formatRate } from "../utils/formatBytes";
import { createRuntimeId } from "../utils/runtimeIds.js";
import { showToast } from "./useToasts";

const logger = createLogger("frontend.sftp.transfers");

const ACTIVE_TRANSFER_STATUSES = new Set(["queued", "running", "pausing", "paused"]);
const TERMINAL_TRANSFER_STATUSES = new Set(["done", "failed", "canceled"]);
const MAX_TERMINAL_TRANSFERS = 50;

function localFileName(path) {
  return (
    String(path || "")
      .split(/[\\/]/)
      .filter(Boolean)
      .pop() || "upload"
  );
}

function makeTransferItem({ name, direction }) {
  return {
    id: createRuntimeId(),
    name,
    direction,
    progress: 0,
    status: "running",
    transferred: 0,
    total: 0,
    speed: "0 B/s",
    error: "",
    lastBytes: 0,
    lastAt: Date.now(),
  };
}

function transferProgress(status, transferred, total) {
  if (status === "done") return 100;
  return total > 0 ? Math.min(99.8, (transferred / total) * 100) : 0;
}

function transferStatusLabel(status, t) {
  switch (status) {
    case "queued":
      return t("sftp.queued");
    case "pausing":
      return t("sftp.pausing");
    case "paused":
      return t("sftp.paused");
    case "done":
      return t("sftp.transferDone");
    case "canceled":
      return t("sftp.canceled");
    case "failed":
      return t("sftp.failed");
    default:
      return "";
  }
}

export function useSftpTransfers({
  props,
  remotePath,
  clearDropTarget = () => {},
  remoteFileByName = { value: new Map() },
  requestUploadConflictAction = () => Promise.resolve(NAME_CONFLICT_ACTION.OVERWRITE),
  selectedEntry,
  selectedNames,
  refreshCurrentDirectoryIncremental = () => {},
  t,
}) {
  const { currentSession, isStaleSession, setDisposed } = createSftpSessionScope(props);
  const transfers = ref([]);
  const activeTransfers = computed(
    () => transfers.value.filter((item) => ACTIVE_TRANSFER_STATUSES.has(item.status)).length,
  );
  const completedTransfers = computed(
    () => transfers.value.filter((item) => item.status === "done").length,
  );

  let unlistenProgress;
  let disposed = false;
  let pendingTransferListRequests = 0;
  let progressEventRevision = 0;
  // 新任务在启动前已入列；只有恢复任务在列表快照前到达的事件需要暂存。
  const pendingProgressPayloads = new Map();
  const flushProgressPayloads = createRafThrottle(() => {
    const payloads = [...pendingProgressPayloads.values()];
    pendingProgressPayloads.clear();
    payloads.forEach((payload) => {
      if (applyTransferProgress(payload) || pendingTransferListRequests === 0) return;
      pendingProgressPayloads.set(payload.transferId, payload);
    });
  });

  // Batch uploads finish many files in quick succession; merge the refreshes
  const scheduleIncrementalRefresh = createDebounced(() => {
    void refreshCurrentDirectoryIncremental();
  }, 500);

  function updateTransferItem(id, patch) {
    // Rebuild the array with a patched copy so the ref change is tracked,
    // but skip no-op patches to avoid useless re-renders and GC pressure
    const index = transfers.value.findIndex((t) => t.id === id);
    if (index < 0) return;
    const existing = transfers.value[index];
    // Skip no-op patches
    let changed = false;
    for (const key of Object.keys(patch)) {
      if (existing[key] !== patch[key]) {
        changed = true;
        break;
      }
    }
    if (!changed) return;
    const next = [...transfers.value];
    next[index] = { ...existing, ...patch };
    transfers.value = next;
    if (patch.status && TERMINAL_TRANSFER_STATUSES.has(patch.status)) {
      pruneTerminalTransfers();
    }
  }

  // 终态条目只保留最近若干条，避免长时间会话里传输列表无限增长；
  // 列表按新到旧排列，超出部分从尾部淘汰，进行中的传输不受影响
  function pruneTerminalTransfers() {
    let terminalSeen = 0;
    const next = transfers.value.filter((item) => {
      if (!TERMINAL_TRANSFER_STATUSES.has(item.status)) return true;
      terminalSeen += 1;
      return terminalSeen <= MAX_TERMINAL_TRANSFERS;
    });
    if (next.length !== transfers.value.length) transfers.value = next;
  }

  function removeTransferItem(id) {
    pendingProgressPayloads.delete(id);
    const index = transfers.value.findIndex((t) => t.id === id);
    if (index < 0) return;
    const next = [...transfers.value];
    next.splice(index, 1);
    transfers.value = next;
  }

  function showTransferError(error, name = "") {
    const message = error?.message || String(error);
    showToast({
      type: "error",
      title: t("sftp.failed"),
      message: name ? `${name}: ${message}` : message,
    });
  }

  async function runTransfer({
    item,
    localPath,
    remotePath: targetRemotePath,
    remoteParentPath,
    remoteName,
    uploadConflictAction = NAME_CONFLICT_ACTION.CREATE,
  }) {
    const session = currentSession();
    if (disposed || !session) return;
    logger.info(
      "transfer.run.started",
      item.direction,
      item.name,
      "->",
      targetRemotePath || `${remoteParentPath}/${remoteName}`,
    );
    const request = {
      connectionId: session.connectionId,
      sessionId: session.sessionId,
      transferId: item.id,
      direction: item.direction,
      localPath,
    };
    if (item.direction === "upload") {
      request.remoteParentPath = remoteParentPath;
      request.remoteName = remoteName;
      request.uploadConflictAction = uploadConflictAction;
    } else {
      request.remotePath = targetRemotePath;
    }
    transfers.value = [item, ...transfers.value];
    try {
      await transferSftp(request);
    } catch (error) {
      if (disposed) return;
      const current = transfers.value.find((transfer) => transfer.id === item.id);
      if (current && !TERMINAL_TRANSFER_STATUSES.has(current.status)) {
        removeTransferItem(item.id);
      }
      logger.error("transfer.run.failed", error);
      showTransferError(error, item.name);
    }
  }

  function transferItemFromBackend(item) {
    const total = Number(item.total || 0);
    const transferred = Number(item.transferred || 0);
    const status = item.status || "paused";
    return {
      id: item.transferId,
      name: item.name,
      direction: item.direction,
      progress: transferProgress(status, transferred, total),
      status,
      transferred,
      total,
      speed: transferStatusLabel(status, t) || "0 B/s",
      error: status === "canceled" ? "" : item.error || "",
      lastBytes: transferred,
      lastAt: Date.now(),
    };
  }

  async function refreshTransferList() {
    if (!props.connection?.id || !props.sessionId) return;
    const connectionId = props.connection.id;
    const sessionId = props.sessionId;
    const eventRevisionAtRequest = progressEventRevision;
    const itemsAtRequest = new Map(transfers.value.map((item) => [item.id, item]));
    pendingTransferListRequests += 1;
    try {
      const backendTransfers = await listSftpTransfers(connectionId, sessionId);
      if (disposed || props.connection?.id !== connectionId || props.sessionId !== sessionId) {
        return;
      }
      flushProgressPayloads.flush();
      const merged = new Map(transfers.value.map((item) => [item.id, item]));
      for (const backendItem of backendTransfers || []) {
        const next = transferItemFromBackend(backendItem);
        const current = merged.get(next.id);
        const previous = itemsAtRequest.get(next.id);
        if (previous && !current) continue;
        const changedSinceRequest = previous && current !== previous;
        const receivedNewerEvent = current && (current.eventRevision || 0) > eventRevisionAtRequest;
        const snapshotIsOlder =
          current &&
          (TERMINAL_TRANSFER_STATUSES.has(current.status) ||
            next.transferred < current.transferred ||
            next.total < current.total);
        if (current && (changedSinceRequest || receivedNewerEvent || snapshotIsOlder)) continue;
        merged.set(next.id, { ...(current || {}), ...next });
      }
      transfers.value = [...merged.values()];
      for (const payload of pendingProgressPayloads.values()) {
        if (applyTransferProgress(payload)) pendingProgressPayloads.delete(payload.transferId);
      }
      pruneTerminalTransfers();
    } catch (error) {
      logger.debug("transfer-list.refresh.failed", error);
    } finally {
      pendingTransferListRequests -= 1;
      if (pendingTransferListRequests === 0) {
        flushProgressPayloads.cancel();
        pendingProgressPayloads.clear();
      }
    }
  }

  function restoreTransferControl(id, pendingItem, restorePatch, error) {
    const current = transfers.value.find((transfer) => transfer.id === id);
    if (current && !TERMINAL_TRANSFER_STATUSES.has(current.status)) {
      updateTransferItem(id, {
        ...(current === pendingItem ? restorePatch : {}),
        error: String(error?.message || error),
      });
    }
    showTransferError(error, current?.name || pendingItem?.name);
  }

  async function pauseTransfer(id) {
    if (disposed) return;
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || !ACTIVE_TRANSFER_STATUSES.has(item.status)) return;
    updateTransferItem(id, { status: "pausing", speed: t("sftp.pausing") });
    const pendingItem = transfers.value.find((transfer) => transfer.id === id);
    try {
      await pauseSftpTransfer(id);
    } catch (error) {
      if (disposed) return;
      restoreTransferControl(id, pendingItem, { status: item.status, speed: item.speed }, error);
    }
  }

  async function resumeTransfer(id) {
    if (disposed) return;
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || item.status !== "paused") return;
    updateTransferItem(id, {
      status: "running",
      speed: "0 B/s",
      error: "",
      lastBytes: item.transferred,
      lastAt: Date.now(),
    });
    const pendingItem = transfers.value.find((transfer) => transfer.id === id);
    try {
      await resumeSftpTransfer(id);
    } catch (error) {
      if (disposed) return;
      restoreTransferControl(
        id,
        pendingItem,
        {
          status: item.status,
          speed: item.speed,
          lastBytes: item.lastBytes,
          lastAt: item.lastAt,
        },
        error,
      );
    }
  }

  async function cancelTransfer(id) {
    if (disposed) return;
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || TERMINAL_TRANSFER_STATUSES.has(item.status)) return;
    try {
      await cancelSftpTransfer(id);
    } catch (error) {
      if (disposed) return;
      restoreTransferControl(id, item, { status: item.status, speed: item.speed }, error);
    }
  }

  async function startDownload(entry = selectedEntry.value) {
    const session = currentSession();
    if (!entry || !session) return;

    let localPath;
    try {
      localPath = await chooseSftpDownloadPath({
        defaultFileName: entry.name,
        kind: entry.kind,
        title: t("sftp.chooseDownloadTitle"),
      });
    } catch (error) {
      if (!disposed && !isStaleSession(session)) showTransferError(error, entry.name);
      return;
    }
    if (disposed || !localPath || isStaleSession(session)) return;

    await runTransfer({
      item: makeTransferItem({ name: entry.name, direction: "download" }),
      localPath,
      remotePath: entry.path,
    });
  }

  async function uploadLocalPaths(
    paths,
    targetDirectory = remotePath.value,
    session = currentSession(),
  ) {
    const normalized = (Array.isArray(paths) ? paths : []).filter(Boolean);
    if (!session || !normalized.length) return;
    if (disposed || isStaleSession(session)) {
      clearDropTarget();
      return;
    }

    const currentDirectory = remotePath.value;
    const canCheckTargetDirectory = targetDirectory === currentDirectory;
    const targetFileByName = await remoteFileMapForUploadTarget(
      targetDirectory,
      canCheckTargetDirectory,
      session,
    );
    if (isStaleSession(session)) {
      clearDropTarget();
      return;
    }
    const uploads = [];
    const uploadTargetNames = new Set();
    for (const localPath of normalized) {
      const name = localFileName(localPath);
      const existing = targetFileByName.get(name);
      let uploadConflictAction = NAME_CONFLICT_ACTION.CREATE;
      if (existing) {
        const conflict = await resolveNameConflict({
          sourcePath: "",
          targetFileByName,
          targetName: name,
          requestConflictAction: requestUploadConflictAction,
          defaultAction: NAME_CONFLICT_ACTION.CREATE,
          skipAction: NAME_CONFLICT_ACTION.SKIP,
        });
        if (isStaleSession(session)) {
          clearDropTarget();
          return;
        }
        uploadConflictAction = conflict.action;
        if (conflict.skipped) continue;
      }
      if (uploadTargetNames.has(name)) {
        showToast({
          type: "error",
          title: t("sftp.failed"),
          message: t("sftp.uploadDuplicateTarget", { name }),
        });
        continue;
      }
      uploadTargetNames.add(name);
      uploads.push({
        localPath,
        name,
        remoteParentPath: targetDirectory,
        uploadConflictAction,
      });
    }
    if (!uploads.length) {
      clearDropTarget();
      return;
    }

    await Promise.all(
      uploads.map(({ localPath, name, remoteParentPath, uploadConflictAction }) => {
        return runTransfer({
          item: makeTransferItem({ name, direction: "upload" }),
          localPath,
          remoteParentPath,
          remoteName: name,
          uploadConflictAction,
        });
      }),
    );
    if (!disposed) clearDropTarget();
  }

  async function remoteFileMapForUploadTarget(targetDirectory, canUseCurrentDirectory, session) {
    if (canUseCurrentDirectory) {
      return remoteFileByName.value || new Map();
    }
    try {
      return await loadRemoteEntriesByName({
        connectionId: session.connectionId,
        sessionId: session.sessionId,
        path: targetDirectory,
      });
    } catch (error) {
      logger.debug("upload-target.remote-files.load.failed", error);
      return new Map();
    }
  }

  async function chooseAndUpload() {
    clearDropTarget();
    const session = currentSession();
    const targetDirectory = remotePath.value;
    if (!session) return;
    let paths;
    try {
      paths = await chooseSftpUploadFiles({
        title: t("sftp.chooseUploadTitle"),
        allFilesLabel: t("recordingDialog.allFiles"),
      });
    } catch (error) {
      if (!disposed && !isStaleSession(session)) showTransferError(error);
      return;
    }
    await uploadLocalPaths(paths, targetDirectory, session);
  }

  async function downloadEntry(entry = selectedEntry.value, closeContextMenu = () => {}) {
    if (!entry) return;
    if (!selectedNames.value.has(entry.name)) {
      selectedNames.value = new Set([entry.name]);
    }
    closeContextMenu();
    await startDownload(entry);
  }

  function applyTransferProgress(payload) {
    const item = transfers.value.find((transfer) => transfer.id === payload?.transferId);
    if (!item) return false;
    logger.debug(
      "transfer.progress",
      payload?.transferId,
      "transferred=",
      payload?.transferred,
      "total=",
      payload?.total,
      "done=",
      payload?.done,
    );

    const now = Date.now();
    const total = Number(payload.total ?? 0);
    const transferred = Number(payload.transferred ?? 0);
    const canceled = payload.status === "canceled" || payload.error === "canceled";
    const status = canceled
      ? "canceled"
      : payload.error
        ? "failed"
        : payload.status || (payload.done ? "done" : "running");
    const done = status === "done";
    const finalTotal = done ? Math.max(total, transferred) : total;
    const finalTransferred = done ? Math.max(total, transferred) : transferred;
    const elapsed = Math.max(1, now - item.lastAt);
    const delta = Math.max(0, transferred - item.lastBytes);

    updateTransferItem(item.id, {
      total: finalTotal,
      transferred: finalTransferred,
      progress: transferProgress(status, finalTransferred, finalTotal),
      speed: transferStatusLabel(status, t) || formatRate(Math.round((delta * 1000) / elapsed)),
      status,
      error: canceled ? "" : payload.error || "",
      eventRevision: payload.eventRevision || item.eventRevision || 0,
      lastBytes: finalTransferred,
      lastAt: now,
    });
    if (done && item.direction === "upload") scheduleIncrementalRefresh();
    return true;
  }

  function updateTransferProgress(payload) {
    if (!payload?.transferId) return;
    progressEventRevision += 1;
    pendingProgressPayloads.set(payload.transferId, {
      ...payload,
      eventRevision: progressEventRevision,
    });
    flushProgressPayloads();
  }

  async function removeTransfer(id) {
    if (disposed) return;
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item) return;
    if (!TERMINAL_TRANSFER_STATUSES.has(item.status)) {
      await cancelTransfer(id);
      return;
    }
    removeTransferItem(id);
  }

  function clearCompletedTransfers() {
    if (disposed) return;
    const next = transfers.value.filter((item) => !TERMINAL_TRANSFER_STATUSES.has(item.status));
    if (next.length === transfers.value.length) return;
    for (const item of transfers.value) {
      if (TERMINAL_TRANSFER_STATUSES.has(item.status)) pendingProgressPayloads.delete(item.id);
    }
    transfers.value = next;
  }

  onMounted(async () => {
    const unlisten = await observeTerminalEvent(
      TERMINAL_EVENTS.SFTP_TRANSFER_PROGRESS,
      updateTransferProgress,
    ).catch((error) => {
      logger.debug("transfer-progress.observe.failed", error);
      return null;
    });
    if (disposed) {
      unlisten?.();
      return;
    }
    unlistenProgress = unlisten || undefined;
    await refreshTransferList();
  });

  watch(
    () => [props.visible, props.connection?.id, props.sessionId],
    ([visible]) => {
      if (visible) refreshTransferList();
    },
  );

  onBeforeUnmount(() => {
    disposed = true;
    setDisposed(true);
    flushProgressPayloads.cancel();
    scheduleIncrementalRefresh.cancel();
    pendingProgressPayloads.clear();
    unlistenProgress?.();
    unlistenProgress = undefined;
  });

  return {
    activeTransfers,
    chooseAndUpload,
    clearCompletedTransfers,
    cancelTransfer,
    completedTransfers,
    downloadEntry,
    pauseTransfer,
    removeTransfer,
    resumeTransfer,
    startDownload,
    transfers,
    uploadLocalPaths,
  };
}
