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
import { createLogger } from "../utils/logger";
import { createDebounced, createRafThrottle } from "../utils/schedulers";
import { formatRate } from "../utils/formatBytes";

const logger = createLogger("frontend.sftp.transfers");

const ACTIVE_TRANSFER_STATUSES = new Set(["running", "pausing", "paused"]);
const TERMINAL_TRANSFER_STATUSES = new Set(["done", "failed", "canceled"]);
const MAX_TERMINAL_TRANSFERS = 50;
const MAX_PENDING_PROGRESS_PAYLOADS = 64;
const UPLOAD_CONFLICT_ACTION = {
  CREATE: "create",
  OVERWRITE: "overwrite",
  RESUME: "resume",
  SKIP: "skip",
};

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
    id: "",
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

export function useSftpTransfers({
  props,
  remotePath,
  clearDropTarget = () => {},
  remoteFileByName = { value: new Map() },
  requestUploadConflictAction = () => Promise.resolve(UPLOAD_CONFLICT_ACTION.OVERWRITE),
  selectedEntry,
  selectedNames,
  refreshCurrentDirectoryIncremental = () => {},
  t,
}) {
  const transfers = ref([]);
  const activeTransfers = computed(
    () => transfers.value.filter((item) => ACTIVE_TRANSFER_STATUSES.has(item.status)).length,
  );
  const completedTransfers = computed(
    () => transfers.value.filter((item) => item.status === "done").length,
  );

  let unlistenProgress;
  let disposed = false;
  // Progress events can arrive before runTransfer registers its transfer id;
  // only re-queue unmatched payloads while such a registration is in flight,
  // otherwise they are trailing/cross-session noise and must be dropped.
  let pendingTransferRegistrations = 0;
  const pendingProgressPayloads = new Map();
  const flushProgressPayloads = createRafThrottle(() => {
    const payloads = [...pendingProgressPayloads.values()];
    pendingProgressPayloads.clear();
    payloads.forEach((payload) => {
      if (applyTransferProgress(payload) || pendingTransferRegistrations === 0) return;
      if (pendingProgressPayloads.size >= MAX_PENDING_PROGRESS_PAYLOADS) {
        pendingProgressPayloads.delete(pendingProgressPayloads.keys().next().value);
      }
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
    const index = transfers.value.findIndex((t) => t.id === id);
    if (index < 0) return;
    const next = [...transfers.value];
    next.splice(index, 1);
    transfers.value = next;
  }

  async function runTransfer({
    item,
    localPath,
    remotePath: targetRemotePath,
    remoteParentPath,
    remoteName,
    uploadConflictAction = UPLOAD_CONFLICT_ACTION.CREATE,
  }) {
    logger.info(
      "transfer.run.started",
      item.direction,
      item.name,
      "->",
      targetRemotePath || `${remoteParentPath}/${remoteName}`,
    );
    try {
      const request = {
        connectionId: props.connection.id,
        sessionId: props.sessionId,
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
      pendingTransferRegistrations += 1;
      const transferId = await transferSftp(request).finally(() => {
        pendingTransferRegistrations -= 1;
      });
      if (disposed) return;
      if (!transferId) {
        throw new Error("SFTP transfer did not return a transfer id.");
      }
      transfers.value = [{ ...item, id: transferId }, ...transfers.value];
      const pending = pendingProgressPayloads.get(transferId);
      if (pending) {
        pendingProgressPayloads.delete(transferId);
        applyTransferProgress(pending);
      }
    } catch (error) {
      logger.error("transfer.run.failed", error);
    }
  }

  function transferItemFromBackend(item) {
    const total = Number(item.total || 0);
    const transferred = Number(item.transferred || 0);
    return {
      id: item.transferId,
      name: item.name,
      direction: item.direction,
      progress: transferProgress(item.status, transferred, total),
      status: item.status || "paused",
      transferred,
      total,
      speed: item.status === "done" ? t("sftp.transferDone") : "0 B/s",
      error: item.error || "",
      lastBytes: transferred,
      lastAt: Date.now(),
    };
  }

  async function refreshTransferList() {
    if (!props.connection?.id || !props.sessionId) return;
    try {
      const backendTransfers = await listSftpTransfers(props.connection.id, props.sessionId);
      if (disposed) return;
      const merged = new Map(transfers.value.map((item) => [item.id, item]));
      for (const backendItem of backendTransfers || []) {
        const next = transferItemFromBackend(backendItem);
        merged.set(next.id, { ...(merged.get(next.id) || {}), ...next });
      }
      transfers.value = [...merged.values()];
      pruneTerminalTransfers();
    } catch (error) {
      logger.debug("transfer-list.refresh.failed", error);
    }
  }

  async function pauseTransfer(id) {
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || !ACTIVE_TRANSFER_STATUSES.has(item.status)) return;
    updateTransferItem(id, { status: "pausing", speed: t("sftp.pausing") });
    try {
      await pauseSftpTransfer(id);
    } catch (error) {
      updateTransferItem(id, {
        status: "failed",
        error: String(error?.message || error),
        speed: t("sftp.failed"),
      });
    }
  }

  async function resumeTransfer(id) {
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || item.status !== "paused") return;
    updateTransferItem(id, {
      status: "running",
      speed: "0 B/s",
      error: "",
      lastBytes: item.transferred,
      lastAt: Date.now(),
    });
    try {
      await resumeSftpTransfer(id);
    } catch (error) {
      updateTransferItem(id, {
        status: "failed",
        error: String(error?.message || error),
        speed: t("sftp.failed"),
      });
    }
  }

  async function cancelTransfer(id) {
    const item = transfers.value.find((transfer) => transfer.id === id);
    if (!item || TERMINAL_TRANSFER_STATUSES.has(item.status)) {
      removeTransfer(id);
      return;
    }
    try {
      await cancelSftpTransfer(id);
      removeTransferItem(id);
    } catch (error) {
      updateTransferItem(id, {
        status: "failed",
        error: String(error?.message || error),
        speed: t("sftp.failed"),
      });
    }
  }

  async function startDownload() {
    const entry = selectedEntry.value;
    if (!entry || !props.connection) return;

    const localPath = await chooseSftpDownloadPath({
      defaultFileName: entry.name,
      kind: entry.kind,
      title: t("sftp.chooseDownloadTitle"),
    });
    if (!localPath) return;

    await runTransfer({
      item: makeTransferItem({ name: entry.name, direction: "download" }),
      localPath,
      remotePath: entry.path,
    });
  }

  async function uploadLocalPaths(paths, targetDirectory = remotePath.value) {
    if (!props.connection) return;
    const normalized = (Array.isArray(paths) ? paths : []).filter(Boolean);
    if (!normalized.length) return;

    const currentDirectory = remotePath.value;
    const canCheckTargetDirectory = targetDirectory === currentDirectory;
    const targetFileByName = await remoteFileMapForUploadTarget(
      targetDirectory,
      canCheckTargetDirectory,
    );
    const uploads = [];
    for (const localPath of normalized) {
      const name = localFileName(localPath);
      const existing = targetFileByName.get(name);
      let uploadConflictAction = UPLOAD_CONFLICT_ACTION.CREATE;
      if (existing && existing.kind !== "dir") {
        const conflict = await resolveNameConflict({
          sourcePath: "",
          targetFileByName,
          targetName: name,
          requestConflictAction: requestUploadConflictAction,
          defaultAction: NAME_CONFLICT_ACTION.CREATE,
          skipAction: NAME_CONFLICT_ACTION.SKIP,
        });
        uploadConflictAction = conflict.action;
        if (conflict.skipped) continue;
      }
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
    clearDropTarget();
  }

  async function remoteFileMapForUploadTarget(targetDirectory, canUseCurrentDirectory) {
    if (canUseCurrentDirectory) {
      return remoteFileByName.value || new Map();
    }
    try {
      return await loadRemoteEntriesByName({
        connectionId: props.connection?.id,
        sessionId: props.sessionId,
        path: targetDirectory,
      });
    } catch (error) {
      logger.debug("upload-target.remote-files.load.failed", error);
      return new Map();
    }
  }

  async function chooseAndUpload() {
    clearDropTarget();
    const paths = await chooseSftpUploadFiles({
      title: t("sftp.chooseUploadTitle"),
      allFilesLabel: t("recordingDialog.allFiles"),
    });
    await uploadLocalPaths(paths, remotePath.value);
  }

  async function downloadEntry(entry = selectedEntry.value, closeContextMenu = () => {}) {
    if (!entry) return;
    if (!selectedNames.value.has(entry.name)) {
      selectedNames.value = new Set([entry.name]);
    }
    closeContextMenu();
    await startDownload();
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

    if (payload.error) {
      const canceled = payload.status === "canceled" || payload.error === "canceled";
      updateTransferItem(item.id, {
        status: canceled ? "canceled" : "failed",
        error: payload.error,
        progress: 100,
        speed: canceled ? t("sftp.canceled") : t("sftp.failed"),
      });
      return true;
    }

    const now = Date.now();
    const total = Number(payload.total ?? 0);
    const transferred = Number(payload.transferred ?? 0);
    const done = !!payload.done;
    const elapsed = Math.max(1, now - item.lastAt);
    const delta = Math.max(0, transferred - item.lastBytes);
    const progress = transferProgress(done ? "done" : payload.status, transferred, total);

    updateTransferItem(item.id, {
      total: done ? Math.max(total, transferred) : total,
      transferred: done ? Math.max(total, transferred) : transferred,
      progress,
      speed:
        payload.status === "paused"
          ? t("sftp.paused")
          : done
            ? t("sftp.transferDone")
            : formatRate(Math.round((delta * 1000) / elapsed)),
      status: payload.status || (done ? "done" : "running"),
      lastBytes: transferred,
      lastAt: now,
    });
    if (done && !payload.error) {
      if (item.direction === "upload") scheduleIncrementalRefresh();
    }
    return true;
  }

  function updateTransferProgress(payload) {
    if (!payload?.transferId) return;
    pendingProgressPayloads.set(payload.transferId, payload);
    flushProgressPayloads();
  }

  async function removeTransfer(id) {
    if (!transfers.value.some((item) => item.id === id)) return;
    try {
      await cancelSftpTransfer(id);
    } catch (error) {
      logger.debug("transfer.remove.cleanup.failed", error);
    }
    removeTransferItem(id);
  }

  async function clearCompletedTransfers() {
    const removable = transfers.value.filter((item) => !ACTIVE_TRANSFER_STATUSES.has(item.status));
    await Promise.all(
      removable.map((item) =>
        cancelSftpTransfer(item.id).catch((error) => {
          logger.debug("transfer.clear-completed.cleanup.failed", error);
        }),
      ),
    );
    const next = transfers.value.filter((item) => ACTIVE_TRANSFER_STATUSES.has(item.status));
    if (next.length === transfers.value.length) return;
    transfers.value = next;
  }

  onMounted(async () => {
    await refreshTransferList();
    const unlisten = await observeTerminalEvent(
      TERMINAL_EVENTS.SFTP_TRANSFER_PROGRESS,
      updateTransferProgress,
    ).catch((error) => {
      logger.debug("transfer-progress.observe.failed", error);
      return null;
    });
    if (!unlisten) return;
    if (disposed) {
      unlisten();
      return;
    }
    unlistenProgress = unlisten;
  });

  watch(
    () => [props.visible, props.connection?.id, props.sessionId],
    ([visible]) => {
      if (visible) refreshTransferList();
    },
  );

  onBeforeUnmount(() => {
    disposed = true;
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
