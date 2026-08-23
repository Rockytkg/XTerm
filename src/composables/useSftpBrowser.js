import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  createRemoteSftpDir,
  createRemoteSftpFile,
  deleteRemoteSftp,
  listRemoteSftp,
} from "../services/sftp";
import { normalizeRemotePath } from "./sftpDragNavigation";
import {
  loadRemoteEntriesByName,
  NAME_CONFLICT_ACTION,
  renameRemoteEntry,
  resolveNameConflict,
} from "./sftpRemoteOperations";
import { createLogger } from "../utils/logger";
import { connectionCan } from "../utils/connectionCapabilities";

const logger = createLogger("frontend.sftp.browser");
const SFTP_ROW_ANIMATION_MS = 700;

function remoteNameFromPath(path) {
  const normalized = normalizeRemotePath(path);
  if (normalized === "/" || normalized === ".") return normalized;
  return normalized.replace(/\/+$/, "").split("/").filter(Boolean).pop() || normalized;
}

function sortRemoteEntries(entries) {
  return [...entries].sort((a, b) => {
    const rankA = a.kind === "dir" ? 0 : 1;
    const rankB = b.kind === "dir" ? 0 : 1;
    return rankA - rankB || a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  });
}

function sameRemoteEntry(previous, next) {
  return (
    previous.name === (next.name || remoteNameFromPath(next.path)) &&
    normalizeRemotePath(previous.path) === normalizeRemotePath(next.path) &&
    previous.kind === next.kind &&
    Number(previous.size || 0) === Number(next.size || 0) &&
    Number(previous.modified || 0) === Number(next.modified || 0)
  );
}

function sameRemoteEntries(previousEntries, nextEntries) {
  if (previousEntries.length !== nextEntries.length) return false;
  for (let index = 0; index < nextEntries.length; index += 1) {
    if (!sameRemoteEntry(previousEntries[index], nextEntries[index])) return false;
  }
  return true;
}

function pathCrumbsFor(path) {
  if (path === ".") return [{ label: ".", path: "." }];
  const absolute = path.startsWith("/");
  const parts = path.split("/").filter(Boolean);
  const crumbs = absolute ? [{ label: "/", path: "/" }] : [];
  let current = "";
  for (const part of parts) {
    current = absolute
      ? `${current}/${part}`.replace(/^\/+/, "/")
      : current
        ? `${current}/${part}`
        : part;
    crumbs.push({ label: part, path: current || "/" });
  }
  return crumbs.length ? crumbs : [{ label: path, path }];
}

export function useSftpBrowser({
  props,
  t,
  closeContextMenu = () => {},
  requestRenameConflictAction = () => Promise.resolve("cancel"),
}) {
  const remotePath = ref(".");
  const remoteParent = ref("");
  const remoteFiles = ref([]);
  const remoteQuery = ref("");
  const selectedNames = ref(new Set());
  const loading = ref(false);
  const pathLoading = ref(false);
  const errorMessage = ref("");
  const editingPath = ref(false);
  const editablePath = ref("");
  const pathInput = ref(null);
  const inlineEditInput = ref(null);
  const inlineEdit = ref({
    active: false,
    mode: "",
    originalPath: "",
    originalName: "",
    originalKind: "dir",
    value: "",
    committing: false,
  });
  const deleteDialog = ref({
    open: false,
    entries: [],
    deleting: false,
  });

  let disposed = false;
  let remoteRequestId = 0;
  let initializedSessionKey = "";
  let reconcileAnimationTimer = 0;

  async function remoteFileMapForPath(path = remotePath.value) {
    const session = currentSession(path);
    if (!session) return new Map();
    const entries = await loadRemoteEntriesByName({
      connectionId: session.connectionId,
      sessionId: session.sessionId,
      path,
    });
    return isStaleSession(session) ? new Map() : entries;
  }

  const filteredRemoteFiles = computed(() => {
    const query = remoteQuery.value.trim().toLowerCase();
    if (!query) return remoteFiles.value;
    return remoteFiles.value.filter((entry) => entry.name.toLowerCase().includes(query));
  });
  const remoteFileByName = computed(
    () => new Map(remoteFiles.value.map((entry) => [entry.name, entry])),
  );
  const creatingFolder = computed(
    () => inlineEdit.value.active && inlineEdit.value.mode === "create-dir",
  );
  const creatingEntry = computed(
    () => inlineEdit.value.active && ["create-dir", "create-file"].includes(inlineEdit.value.mode),
  );
  const selectedEntries = computed(() => {
    const entries = [];
    const byName = remoteFileByName.value;
    for (const name of selectedNames.value) {
      const entry = byName.get(name);
      if (entry) entries.push(entry);
    }
    return entries;
  });
  const selectedEntry = computed(() => selectedEntries.value[0] || null);
  const canDownload = computed(() => selectedEntries.value.length === 1);
  const pathCrumbs = computed(() => pathCrumbsFor(remotePath.value));

  function clearSelection() {
    selectedNames.value = new Set();
  }

  function normalizeEntry(entry, animation = "") {
    return {
      ...entry,
      name: entry.name || remoteNameFromPath(entry.path),
      animation,
    };
  }

  function clearReconcileAnimations() {
    if (reconcileAnimationTimer) {
      window.clearTimeout(reconcileAnimationTimer);
      reconcileAnimationTimer = 0;
    }
    if (remoteFiles.value.some((entry) => entry.animation)) {
      remoteFiles.value = remoteFiles.value.map((entry) =>
        entry.animation ? { ...entry, animation: "" } : entry,
      );
    }
  }

  function scheduleClearReconcileAnimations() {
    if (reconcileAnimationTimer) window.clearTimeout(reconcileAnimationTimer);
    reconcileAnimationTimer = window.setTimeout(() => {
      reconcileAnimationTimer = 0;
      clearReconcileAnimations();
    }, SFTP_ROW_ANIMATION_MS);
  }

  function reconcileRemoteEntries(nextEntries, options = {}) {
    if (sameRemoteEntries(remoteFiles.value, nextEntries || [])) {
      return false;
    }

    const animate = options.animate !== false;
    const previousByPath = new Map(
      remoteFiles.value.map((entry) => [normalizeRemotePath(entry.path), entry]),
    );
    const next = [];
    let changed = remoteFiles.value.length !== nextEntries.length;

    for (const rawEntry of nextEntries || []) {
      const path = normalizeRemotePath(rawEntry.path);
      const previous = previousByPath.get(path);
      previousByPath.delete(path);
      if (!previous) {
        changed = true;
        next.push(normalizeEntry(rawEntry, animate ? "added" : ""));
      } else if (sameRemoteEntry(previous, rawEntry)) {
        next.push(previous.animation ? { ...previous, animation: "" } : previous);
      } else {
        changed = true;
        next.push(normalizeEntry(rawEntry, animate ? "updated" : ""));
      }
    }

    if (previousByPath.size) changed = true;
    if (!changed) return false;

    remoteFiles.value = sortRemoteEntries(next);
    if (options.clearSelection !== false) {
      const names = new Set(remoteFiles.value.map((entry) => entry.name));
      selectedNames.value = new Set([...selectedNames.value].filter((name) => names.has(name)));
    }
    if (options.selectName) {
      selectedNames.value = new Set([options.selectName]);
    }
    if (animate) scheduleClearReconcileAnimations();
    return true;
  }

  async function refreshCurrentDirectoryIncremental(options = {}) {
    return refreshRemote(remotePath.value || ".", {
      ...options,
      incremental: true,
      preserveEntries: true,
      suppressError: options.suppressError ?? true,
    });
  }

  function selectEntry(entry, event) {
    if (inlineEdit.value.active) return;
    if (event?.ctrlKey || event?.metaKey) {
      const next = new Set(selectedNames.value);
      next.has(entry.name) ? next.delete(entry.name) : next.add(entry.name);
      selectedNames.value = next;
      return;
    }
    if (selectedNames.value.has(entry.name)) {
      selectedNames.value = new Set();
      return;
    }
    selectedNames.value = new Set([entry.name]);
  }

  function uniqueEntryName(base) {
    const existing = remoteFileByName.value;
    if (!existing.has(base)) return base;
    let index = 2;
    while (existing.has(`${base}-${index}`)) index += 1;
    return `${base}-${index}`;
  }

  function selectInlineEditText() {
    const input = Array.isArray(inlineEditInput.value)
      ? inlineEditInput.value[0]
      : inlineEditInput.value;
    if (!input) return;
    input.focus();
    const value = input.value;
    if (inlineEdit.value.mode === "rename" && inlineEdit.value.originalKind !== "dir") {
      const dot = value.lastIndexOf(".");
      if (dot > 0) {
        input.setSelectionRange(0, dot);
        return;
      }
    }
    input.select();
  }

  function startCreateFolder() {
    if (!props.connection || inlineEdit.value.active) return;
    closeContextMenu();
    inlineEdit.value = {
      active: true,
      mode: "create-dir",
      originalPath: "",
      originalName: "",
      originalKind: "dir",
      value: uniqueEntryName(t("sftp.newFolderDefault")),
      committing: false,
    };
    clearSelection();
    nextTick(selectInlineEditText);
  }

  function startCreateFile() {
    if (!props.connection || inlineEdit.value.active) return;
    closeContextMenu();
    inlineEdit.value = {
      active: true,
      mode: "create-file",
      originalPath: "",
      originalName: "",
      originalKind: "file",
      value: uniqueEntryName(t("sftp.newFileDefault")),
      committing: false,
    };
    clearSelection();
    nextTick(selectInlineEditText);
  }

  function startRenameEntry(entry = selectedEntry.value) {
    if (!entry || !props.connection || inlineEdit.value.active) return;
    closeContextMenu();
    selectedNames.value = new Set([entry.name]);
    inlineEdit.value = {
      active: true,
      mode: "rename",
      originalPath: entry.path,
      originalName: entry.name,
      originalKind: entry.kind,
      value: entry.name,
      committing: false,
    };
    nextTick(selectInlineEditText);
  }

  function cancelInlineEdit() {
    inlineEdit.value = {
      active: false,
      mode: "",
      originalPath: "",
      originalName: "",
      originalKind: "dir",
      value: "",
      committing: false,
    };
  }

  function isEditingEntry(entry) {
    return (
      inlineEdit.value.active &&
      inlineEdit.value.mode === "rename" &&
      inlineEdit.value.originalPath === entry.path
    );
  }

  async function commitInlineEdit() {
    if (!inlineEdit.value.active || inlineEdit.value.committing || !props.connection) return;
    const session = currentSession(remotePath.value);
    if (!session) return;
    const edit = inlineEdit.value;
    const nextName = edit.value.trim();
    if (!nextName) {
      cancelInlineEdit();
      return;
    }
    if (nextName.includes("/") || nextName.includes("\\")) {
      errorMessage.value = t("sftp.invalidName");
      nextTick(selectInlineEditText);
      return;
    }
    if (edit.mode === "rename" && nextName === edit.originalName) {
      cancelInlineEdit();
      return;
    }

    inlineEdit.value = { ...edit, value: nextName, committing: true };
    try {
      if (edit.mode === "create-dir") {
        await createRemoteSftpDir(session.connectionId, session.sessionId, session.path, nextName);
      } else if (edit.mode === "create-file") {
        await createRemoteSftpFile(session.connectionId, session.sessionId, session.path, nextName);
      } else if (edit.mode === "rename") {
        let conflictAction = NAME_CONFLICT_ACTION.CREATE;
        const currentEntry = remoteFileByName.value.get(edit.originalName);
        const conflict = await resolveNameConflict({
          sourcePath: edit.originalPath,
          sourceEntry: currentEntry || null,
          targetFileByName: remoteFileByName.value,
          targetName: nextName,
          requestConflictAction: requestRenameConflictAction,
          defaultAction: NAME_CONFLICT_ACTION.CREATE,
          skipAction: NAME_CONFLICT_ACTION.CANCEL,
        });
        conflictAction = conflict.action;
        if (isStaleSession(session)) return;
        if (conflict.cancelled) {
          inlineEdit.value = { ...inlineEdit.value, committing: false };
          nextTick(selectInlineEditText);
          return;
        }
        await renameRemoteEntry({
          connectionId: session.connectionId,
          sessionId: session.sessionId,
          fromPath: edit.originalPath,
          toParentPath: session.path,
          toName: nextName,
          conflictAction,
        });
      }
      if (isStaleSession(session)) return;
      cancelInlineEdit();
      await refreshCurrentDirectoryIncremental();
    } catch (error) {
      if (isStaleSession(session)) return;
      errorMessage.value = String(error?.message || error);
      inlineEdit.value = { ...inlineEdit.value, committing: false };
      nextTick(selectInlineEditText);
    }
  }

  function startPathEdit() {
    if (editingPath.value) return;
    editablePath.value = remotePath.value;
    editingPath.value = true;
    nextTick(() => {
      pathInput.value?.focus();
      pathInput.value?.select();
    });
  }

  function cancelPathEdit() {
    editablePath.value = remotePath.value;
    editingPath.value = false;
  }

  function submitPathEdit() {
    const nextPath = editablePath.value.trim();
    editingPath.value = false;
    if (nextPath && nextPath !== remotePath.value) {
      refreshRemote(nextPath);
    }
  }

  async function initializeRemotePath() {
    const path = props.workingDirectory || remotePath.value || ".";
    const loaded = await refreshRemote(path, { pathLoading: true, suppressError: true });
    if (!loaded && !disposed) {
      await refreshRemote(".", { pathLoading: true });
    }
  }

  async function initializeVisibleRemotePath() {
    if (!props.visible) return;
    const sessionKey = `${props.connection?.id || ""}:${props.sessionId || ""}`;
    if (!sessionKey || sessionKey === ":") return;
    if (initializedSessionKey === sessionKey) return;
    await initializeRemotePath();
    if (!disposed) initializedSessionKey = sessionKey;
  }

  function createRemoteRequest(path) {
    if (!connectionCan(props.connection, "sftp") || !props.sessionId) return;
    return {
      id: ++remoteRequestId,
      connectionId: props.connection.id,
      sessionId: props.sessionId,
      path,
    };
  }

  function currentSession(path = remotePath.value) {
    if (!connectionCan(props.connection, "sftp") || !props.sessionId) return null;
    return {
      connectionId: props.connection.id,
      sessionId: props.sessionId,
      path,
    };
  }

  function isStaleSession(session) {
    return (
      disposed ||
      !session ||
      props.connection?.id !== session.connectionId ||
      props.sessionId !== session.sessionId
    );
  }

  function isStaleRemoteRequest(request) {
    return (
      disposed ||
      !request ||
      request.id !== remoteRequestId ||
      props.connection?.id !== request.connectionId ||
      props.sessionId !== request.sessionId
    );
  }

  async function refreshRemote(path = remotePath.value || ".", options = {}) {
    const request = createRemoteRequest(path);
    if (!request) return false;
    logger.debug("remote.refresh.start", {
      path,
      connectionId: request.connectionId,
      sessionId: request.sessionId,
    });

    const preserveEntries = options.preserveEntries === true;
    if (!preserveEntries) loading.value = true;
    if (options.pathLoading) pathLoading.value = true;
    errorMessage.value = "";
    try {
      const result = await listRemoteSftp(request.connectionId, request.sessionId, request.path);
      if (isStaleRemoteRequest(request)) return false;

      remotePath.value = result.path || request.path;
      remoteParent.value = result.parent || "";
      const entries = Array.isArray(result.entries) ? result.entries : [];
      if (options.incremental) {
        reconcileRemoteEntries(entries, options);
      } else {
        clearReconcileAnimations();
        remoteFiles.value = entries.map((entry) => normalizeEntry(entry));
        clearSelection();
      }
      return true;
    } catch (error) {
      if (isStaleRemoteRequest(request)) return false;
      if (!options.suppressError) {
        errorMessage.value = String(error?.message || error);
      }
      return false;
    } finally {
      if (!isStaleRemoteRequest(request)) {
        if (!preserveEntries) loading.value = false;
        if (options.pathLoading) pathLoading.value = false;
      }
    }
  }

  function openEntry(entry) {
    if (entry.kind === "dir") {
      refreshRemote(entry.path);
    } else {
      selectedNames.value = new Set([entry.name]);
    }
  }

  function requestDeleteEntries(entries) {
    if (!entries.length || !props.connection) return;
    closeContextMenu();
    deleteDialog.value = {
      open: true,
      entries: [...entries],
      deleting: false,
    };
  }

  function closeDeleteDialog() {
    if (deleteDialog.value.deleting) return;
    // 保留 entries：退出动画仍在渲染删除列表，清空会导致内容先于弹壳消失
    deleteDialog.value = { ...deleteDialog.value, open: false };
  }

  async function confirmDeleteEntries() {
    const entries = deleteDialog.value.entries;
    if (!entries.length || !props.connection || deleteDialog.value.deleting) return;
    const session = currentSession(remotePath.value);
    if (!session) return;
    logger.info("remote.delete.requested", {
      count: entries.length,
      paths: entries.map((entry) => entry.path),
    });
    deleteDialog.value = { ...deleteDialog.value, deleting: true };
    try {
      await deleteRemoteSftp(
        session.connectionId,
        session.sessionId,
        entries.map((entry) => entry.path),
      );
      if (isStaleSession(session)) return;
      // 同 closeDeleteDialog：保留 entries 直至退出动画结束
      deleteDialog.value = { ...deleteDialog.value, open: false, deleting: false };
      await refreshCurrentDirectoryIncremental();
    } catch (error) {
      if (isStaleSession(session)) return;
      errorMessage.value = String(error?.message || error);
      deleteDialog.value = { ...deleteDialog.value, deleting: false };
    }
  }

  function resetBrowser() {
    remoteRequestId += 1;
    initializedSessionKey = "";
    remotePath.value = ".";
    remoteQuery.value = "";
    remoteFiles.value = [];
    clearSelection();
  }

  onMounted(() => {
    disposed = false;
    initializeVisibleRemotePath();
  });

  watch([() => props.connection?.id, () => props.sessionId], () => {
    resetBrowser();
    initializeVisibleRemotePath();
  });

  watch(
    () => props.visible,
    (visible) => {
      if (visible) {
        initializeVisibleRemotePath();
      }
    },
  );

  onBeforeUnmount(() => {
    disposed = true;
    remoteRequestId += 1;
    if (reconcileAnimationTimer) {
      window.clearTimeout(reconcileAnimationTimer);
      reconcileAnimationTimer = 0;
    }
  });

  return {
    canDownload,
    cancelInlineEdit,
    cancelPathEdit,
    closeDeleteDialog,
    commitInlineEdit,
    confirmDeleteEntries,
    creatingEntry,
    creatingFolder,
    deleteDialog,
    editablePath,
    editingPath,
    errorMessage,
    filteredRemoteFiles,
    inlineEdit,
    inlineEditInput,
    isEditingEntry,
    loading,
    openEntry,
    pathCrumbs,
    pathInput,
    pathLoading,
    refreshRemote,
    refreshCurrentDirectoryIncremental,
    remoteFiles,
    remoteFileByName,
    remoteParent,
    remotePath,
    remoteQuery,
    requestDeleteEntries,
    remoteFileMapForPath,
    selectEntry,
    selectedEntries,
    selectedEntry,
    selectedNames,
    startCreateFile,
    startCreateFolder,
    startPathEdit,
    startRenameEntry,
    submitPathEdit,
  };
}
