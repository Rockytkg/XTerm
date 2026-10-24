import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import {
  createRemoteSftpDir,
  createRemoteSftpFile,
  deleteRemoteSftp,
  listRemoteSftp,
  renameRemoteSftp,
} from "../services/sftp";
import { normalizeRemotePath } from "./sftpDragNavigation";
import {
  loadRemoteEntriesByName,
  NAME_CONFLICT_ACTION,
  remoteNameFromPath,
  resolveNameConflict,
} from "./sftpRemoteOperations";
import { createSftpSessionScope } from "./sftpSessionScope";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.sftp.browser");
const SFTP_ROW_ANIMATION_MS = 700;

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
  // 大列表/集合只在整体替换时触发更新（写入点均为 .value 赋新值），
  // 用 shallowRef 避免对成百上千个 entry 做深度响应式代理。
  const remoteFiles = shallowRef([]);
  const remoteQuery = ref("");
  const selectedNames = shallowRef(new Set());
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

  let remoteRequestId = 0;
  let reconcileAnimationTimer = 0;

  const { currentSession, isDisposed, isStaleSession, setDisposed } = createSftpSessionScope(props);

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

  function reconcileRemoteEntries(nextEntries) {
    if (sameRemoteEntries(remoteFiles.value, nextEntries || [])) {
      return false;
    }

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
        next.push(normalizeEntry(rawEntry, "added"));
      } else if (sameRemoteEntry(previous, rawEntry)) {
        next.push(previous.animation ? { ...previous, animation: "" } : previous);
      } else {
        changed = true;
        next.push(normalizeEntry(rawEntry, "updated"));
      }
    }

    if (previousByPath.size) changed = true;
    if (!changed) return false;

    remoteFiles.value = next;
    const names = new Set(remoteFiles.value.map((entry) => entry.name));
    selectedNames.value = new Set([...selectedNames.value].filter((name) => names.has(name)));
    scheduleClearReconcileAnimations();
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
    if (edit.mode === "rename" && edit.value === edit.originalName) {
      cancelInlineEdit();
      return;
    }
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
        await renameRemoteSftp(
          session.connectionId,
          session.sessionId,
          edit.originalPath,
          session.path,
          nextName,
          conflictAction,
        );
      }
      if (isStaleSession(session)) return;
      cancelInlineEdit();
      await refreshCurrentDirectoryIncremental({ suppressError: false });
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
    if (!editingPath.value) return;
    const nextPath = editablePath.value.trim();
    editingPath.value = false;
    if (nextPath && nextPath !== remotePath.value) {
      void refreshRemote(nextPath);
    }
  }

  function remoteSessionKey() {
    return `${props.connection?.id || ""}:${props.sessionId || ""}`;
  }

  async function initializeRemotePath() {
    const sessionKey = remoteSessionKey();
    if (sessionKey === ":") return null;
    const path = props.workingDirectory || remotePath.value || ".";
    const requestPromise = refreshRemote(path, { pathLoading: true, suppressError: true });
    const requestId = remoteRequestId;
    const loaded = await requestPromise;
    if (
      loaded !== false ||
      requestId !== remoteRequestId ||
      sessionKey !== remoteSessionKey() ||
      isDisposed()
    ) {
      return loaded;
    }
    return refreshRemote(".", { pathLoading: true });
  }

  function createRemoteRequest(path) {
    const session = currentSession(path);
    if (!session) return null;
    return { ...session, id: ++remoteRequestId };
  }

  function isStaleRemoteRequest(request) {
    return !request || request.id !== remoteRequestId || isStaleSession(request);
  }

  async function refreshRemote(path = remotePath.value || ".", options = {}) {
    const request = createRemoteRequest(path);
    if (!request) return null;
    logger.debug("remote.refresh.start", {
      path,
      connectionId: request.connectionId,
      sessionId: request.sessionId,
    });

    const preserveEntries = options.preserveEntries === true;
    loading.value = !preserveEntries;
    pathLoading.value = options.pathLoading === true;
    errorMessage.value = "";
    try {
      const result = await listRemoteSftp(request.connectionId, request.sessionId, request.path);
      if (isStaleRemoteRequest(request)) return null;

      remotePath.value = result.path || request.path;
      remoteParent.value = result.parent || "";
      const entries = Array.isArray(result.entries) ? result.entries : [];
      if (options.incremental) {
        reconcileRemoteEntries(entries);
      } else {
        clearReconcileAnimations();
        remoteFiles.value = entries.map((entry) => normalizeEntry(entry));
        clearSelection();
      }
      return true;
    } catch (error) {
      if (isStaleRemoteRequest(request)) return null;
      if (!options.suppressError) {
        errorMessage.value = String(error?.message || error);
      }
      return false;
    } finally {
      if (request.id === remoteRequestId) {
        loading.value = false;
        pathLoading.value = false;
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
    let deleteError = "";
    try {
      await deleteRemoteSftp(
        session.connectionId,
        session.sessionId,
        entries.map((entry) => entry.path),
      );
    } catch (error) {
      deleteError = String(error?.message || error);
    }
    if (isStaleSession(session)) return;

    // 批量删除可能已部分成功；失败也要同步真实剩余项，且不能让刷新覆盖原始删除错误。
    deleteDialog.value = { ...deleteDialog.value, open: false, deleting: false };
    await refreshCurrentDirectoryIncremental({ suppressError: Boolean(deleteError) });
    if (deleteError) errorMessage.value = deleteError;
  }

  function resetBrowser() {
    remoteRequestId += 1;
    clearReconcileAnimations();
    remotePath.value = ".";
    remoteParent.value = "";
    remoteQuery.value = "";
    remoteFiles.value = [];
    loading.value = false;
    pathLoading.value = false;
    errorMessage.value = "";
    editingPath.value = false;
    editablePath.value = "";
    deleteDialog.value = { ...deleteDialog.value, open: false, deleting: false };
    cancelInlineEdit();
    clearSelection();
  }

  onMounted(() => {
    setDisposed(false);
    void initializeRemotePath();
  });

  watch([() => props.connection?.id, () => props.sessionId], () => {
    resetBrowser();
    void initializeRemotePath();
  });

  onBeforeUnmount(() => {
    setDisposed(true);
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
