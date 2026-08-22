<script setup>
import { computed, defineAsyncComponent, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { AlertCircle, File, Folder, X } from "@lucide/vue";
import { TabsList, TabsRoot, TabsTrigger } from "reka-ui";
import { storeToRefs } from "pinia";
import ConfirmDialog from "./ConfirmDialog.vue";
import SftpFileTable from "./sftp/SftpFileTable.vue";
import SftpPathBar from "./sftp/SftpPathBar.vue";
import "../styles/sftp.scss";
const CodeEditor = defineAsyncComponent(() => import("./CodeEditor.vue"));
import SftpToolbar from "./sftp/SftpToolbar.vue";
import SftpTransferQueue from "./sftp/SftpTransferQueue.vue";
import { useSftpBrowser } from "../composables/useSftpBrowser";
import { useSftpDialogStates } from "../composables/useSftpDialogStates";
import { useSftpDragDrop } from "../composables/useSftpDragDrop";
import { useSftpEditors } from "../composables/useSftpEditors";
import { useSftpMotion } from "../composables/useSftpMotion";
import { useSftpMoveDrag } from "../composables/useSftpMoveDrag";
import { useSftpTransfers } from "../composables/useSftpTransfers";
import { dismissContextMenu, openContextMenu } from "../services/contextMenu";
import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { closeSftpSession } from "../services/sftp";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { resolveEditorTheme } from "../utils/editorTheme";
import { blurActiveElement } from "../utils/focusGuards";
import { createShortcutRegistry } from "../utils/shortcutRegistry";
import { createLogger } from "../utils/logger";

const props = defineProps({
  connection: { type: Object, default: null },
  sessionId: { type: String, default: "" },
  workingDirectory: { type: String, default: "/" },
  visible: { type: Boolean, default: false },
});

const { t, locale } = useI18n();
const { preferences, resolvedTheme } = storeToRefs(useWorkspaceStore());
const logger = createLogger("frontend.sftp.panel");

const editorMode = ref(false);
const queueCollapsed = ref(true);
const queueListRef = ref(null);

const endpointLabel = computed(() => {
  if (!props.connection) return "-";
  const host = props.connection.host || props.connection.port || props.connection.name || "-";
  return `${props.connection.user ? `${props.connection.user}@` : ""}${host}`;
});

// 虚拟列表滚动会对同一秒级时间戳反复触发格式化，缓存结果避免重复 toLocaleString
const MODIFIED_TIME_CACHE_LIMIT = 256;
const modifiedTimeCache = new Map();

function formatModified(seconds) {
  if (!seconds) return "-";
  const cached = modifiedTimeCache.get(seconds);
  if (cached !== undefined) return cached;
  const formatted = new Date(seconds * 1000).toLocaleString();
  if (modifiedTimeCache.size >= MODIFIED_TIME_CACHE_LIMIT) {
    // Map 按插入序迭代，淘汰最旧的条目
    modifiedTimeCache.delete(modifiedTimeCache.keys().next().value);
  }
  modifiedTimeCache.set(seconds, formatted);
  return formatted;
}

// 语言切换会改变本地化日期格式，缓存随之失效
watch(locale, () => {
  modifiedTimeCache.clear();
});

function fileTypeLabel(entry) {
  if (entry.kind === "dir") return t("sftp.folder");
  if (entry.kind === "symlink") return t("sftp.symlink");
  return t("sftp.file");
}

async function copyText(text) {
  if (!text) return;
  await writeClipboardText(text);
  closeContextMenu();
}

function closeContextMenu() {
  dismissContextMenu();
}

const {
  conflictDialog,
  dirtyEditorDialog,
  editorConfirmDescription,
  editorConfirmSecondaryText,
  editorConfirmText,
  editorConfirmTitle,
  onConflictDialogOpenChange,
  onDirtyEditorDialogOpenChange,
  requestRenameConflictAction,
  requestDirtyEditorAction,
  requestOverwriteEditorAction,
  requestUploadConflictAction,
  resolveConflictAction,
  resolveDirtyEditorAction,
} = useSftpDialogStates({ t });

const {
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
  remoteFileMapForPath,
  remoteParent,
  remotePath,
  remoteQuery,
  requestDeleteEntries: requestBrowserDeleteEntries,
  selectEntry,
  selectedEntries,
  selectedEntry,
  selectedNames,
  startCreateFile,
  startCreateFolder,
  startPathEdit,
  startRenameEntry,
  submitPathEdit,
} = useSftpBrowser({
  props,
  t,
  closeContextMenu,
  requestRenameConflictAction,
});

const hasEditorTabs = computed(() => editorTabs.value.length > 0);
const currentEditorDirty = computed(
  () =>
    !!activeEditorTab.value && activeEditorTab.value.content !== activeEditorTab.value.savedContent,
);
const parentDirectoryEntry = computed(() =>
  remoteParent.value
    ? {
        name: "..",
        kind: "dir",
        path: remoteParent.value,
        size: 0,
        modified: 0,
      }
    : null,
);
const fileTableLabels = computed(() => ({
  emptyFolder: t("sftp.emptyFolder"),
  file: t("sftp.file"),
  folder: t("sftp.folder"),
  loading: t("sftp.loading"),
  modified: t("sftp.modified"),
  name: t("sftp.name"),
  newFile: t("sftp.context.newFile"),
  newFolder: t("sftp.context.newFolder"),
  rename: t("sftp.context.rename"),
  size: t("sftp.size"),
  type: t("sftp.type"),
}));
const resolvedEditorTheme = computed(() =>
  resolveEditorTheme(preferences.value.editorThemeMode, resolvedTheme.value),
);
const deleteEntriesSummary = computed(() => {
  const entries = deleteDialog.value.entries;
  const folders = entries.filter((entry) => entry.kind === "dir").length;
  const files = entries.length - folders;
  return { files, folders };
});
const deleteDialogTitle = computed(() => {
  const entries = deleteDialog.value.entries;
  if (entries.length !== 1) return t("sftp.deleteTitle");
  return entries[0]?.kind === "dir" ? t("sftp.deleteFolderTitle") : t("sftp.deleteFileTitle");
});
const deleteDialogDescription = computed(() => {
  const entries = deleteDialog.value.entries;
  if (entries.length === 1) {
    const name = entries[0]?.name || "";
    return entries[0]?.kind === "dir"
      ? t("sftp.deleteOneFolderMessage", { name })
      : t("sftp.deleteOneFileMessage", { name });
  }
  return t("sftp.deleteManyTypedMessage", {
    count: entries.length,
    files: deleteEntriesSummary.value.files,
    folders: deleteEntriesSummary.value.folders,
  });
});

function requestDeleteEntries(entries = selectedEntries.value) {
  releaseSftpDeleteSourceFocus();
  requestBrowserDeleteEntries(entries);
}

function releaseSftpDeleteSourceFocus() {
  blurActiveElement({ within: ".sftp-root" });
}

function isEditableContextTarget(target) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target?.isContentEditable
  );
}

function contextMenuItem(id, label, icon, enabled, action, options = {}) {
  return {
    id,
    label,
    icon,
    enabled,
    action,
    ...options,
  };
}

function contextMenuSeparator() {
  return { type: "separator" };
}

function entryFromContextEvent(nativeEvent) {
  const row = nativeEvent.target?.closest?.(".sftp-row[data-path]");
  const path = row?.dataset?.path;
  return path ? remoteFiles.value.find((entry) => entry.path === path) || null : null;
}

function entryListText(entries, field) {
  return entries
    .map((entry) => entry?.[field])
    .filter(Boolean)
    .join("\n");
}

function selectedDocumentText() {
  return String(window.getSelection?.()?.toString() || "");
}

async function copySelectionText(entries, field) {
  if (!entries.length) return;
  await copyText(entryListText(entries, field));
}

function buildBlankContextMenuItems() {
  return [
    contextMenuItem("sftp-upload", t("sftp.upload"), "upload", true, chooseAndUpload),
    contextMenuSeparator(),
    contextMenuItem("sftp-new-file", t("sftp.context.newFile"), "newFile", true, startCreateFile),
    contextMenuItem(
      "sftp-new-folder",
      t("sftp.context.newFolder"),
      "newFolder",
      true,
      startCreateFolder,
    ),
    contextMenuSeparator(),
    contextMenuItem(
      "sftp-select-all",
      t("sftp.context.selectAll"),
      "selectAll",
      filteredRemoteFiles.value.length > 0,
      selectAllFiles,
      { shortcut: "Ctrl+A" },
    ),
    contextMenuItem("sftp-refresh", t("sftp.refresh"), "refresh", true, () => refreshRemote()),
  ];
}

function buildSftpContextMenuItems(entry, selection) {
  const contextEntry = entry || selection[0] || null;
  if (!contextEntry) return buildBlankContextMenuItems();
  const canRename = selection.length === 1;
  const canEdit = selection.length === 1 && contextEntry?.kind !== "dir";
  const hasSelection = selection.length > 0;

  return [
    ...(contextEntry?.kind === "dir" && selection.length === 1
      ? [
          contextMenuItem("sftp-open", t("sftp.context.open"), "open", true, () =>
            openEntry(contextEntry),
          ),
        ]
      : []),
    contextMenuItem("sftp-download", t("sftp.download"), "download", selection.length === 1, () =>
      downloadEntry(selection[0]),
    ),
    contextMenuItem("sftp-edit", t("sftp.context.edit"), "edit", canEdit, () =>
      openEditorFromContext(selection[0]),
    ),
    contextMenuSeparator(),
    contextMenuItem(
      "sftp-rename",
      t("sftp.context.rename"),
      "rename",
      canRename,
      () => startRenameEntry(selection[0]),
      { shortcut: "F2" },
    ),
    contextMenuItem(
      "sftp-delete",
      t("sftp.context.delete"),
      "delete",
      hasSelection,
      () => requestDeleteEntries(selection),
      { shortcut: "Del", tone: "danger" },
    ),
    contextMenuSeparator(),
    contextMenuItem(
      "sftp-copy",
      t("sftp.context.copy"),
      "copy",
      hasSelection,
      () => copySelectionText(selection, "name"),
      { shortcut: "Ctrl+C" },
    ),
    contextMenuItem(
      "sftp-copy-path",
      selection.length > 1 ? t("sftp.context.copyPaths") : t("sftp.context.copyPath"),
      "copyPath",
      hasSelection,
      () => copySelectionText(selection, "path"),
    ),
    contextMenuItem(
      "sftp-select-all",
      t("sftp.context.selectAll"),
      "selectAll",
      filteredRemoteFiles.value.length > 0,
      selectAllFiles,
      { shortcut: "Ctrl+A" },
    ),
    contextMenuItem("sftp-refresh", t("sftp.refresh"), "refresh", true, () => refreshRemote()),
  ];
}

async function provideContextMenu(event) {
  if (!props.visible) return;
  if (isEditableContextTarget(event.target)) return;

  const entry = entryFromContextEvent(event);
  if (!entry && selectedDocumentText()) {
    await openContextMenu(event);
    return;
  }

  if (entry && !selectedNames.value.has(entry.name)) {
    selectedNames.value = new Set([entry.name]);
  } else if (!entry && !selectedEntries.value.length) {
    selectedNames.value = new Set();
  }
  const selection = [...selectedEntries.value];
  await openContextMenu(event, {
    suppressDefaultEditItems: true,
    items: buildSftpContextMenuItems(entry, selection),
  });
}

function closeDeleteDialogOnDismiss(value) {
  if (!value) closeDeleteDialog();
}

function selectAllFiles() {
  selectedNames.value = new Set(filteredRemoteFiles.value.map((entry) => entry.name));
  closeContextMenu();
}

function clearSelectionOnBlankClick(event) {
  if (inlineEdit.value.active) return;
  if (event.target?.closest?.(".sftp-row")) return;
  selectedNames.value = new Set();
  closeContextMenu();
}

function sftpShortcutScopeEnabled(event) {
  if (!props.visible) return false;
  return !(event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement);
}

const sftpShortcuts = createShortcutRegistry();
sftpShortcuts.register({
  id: "sftp.escape",
  shortcut: "Escape",
  when: sftpShortcutScopeEnabled,
  preventDefault: false,
  consume: false,
  run: () => {
    closeContextMenu();
    if (inlineEdit.value.active) cancelInlineEdit();
    if (deleteDialog.value.open) closeDeleteDialog();
  },
});
sftpShortcuts.register({
  id: "sftp.delete",
  shortcut: "Delete",
  when: (event) => sftpShortcutScopeEnabled(event) && selectedEntries.value.length > 0,
  run: () => requestDeleteEntries(selectedEntries.value),
});
sftpShortcuts.register({
  id: "sftp.rename",
  shortcut: "F2",
  when: (event) => sftpShortcutScopeEnabled(event) && selectedEntries.value.length === 1,
  run: () => startRenameEntry(selectedEntries.value[0]),
});

watch(
  () => props.visible,
  (visible, _, onCleanup) => {
    if (!visible) {
      return;
    }

    sftpShortcuts.attach(document);
    onCleanup(() => {
      sftpShortcuts.detach();
    });
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  if (props.connection?.id && props.sessionId) {
    closeSftpSession(props.connection.id, props.sessionId).catch((error) => {
      logger.debug("session.close.failed", error);
    });
  }
});

const { dragActive, dropTargetPath, onTableDomDragLeave, onTableDomDragOver, onTableDomDrop } =
  useSftpDragDrop({
    props,
    remotePath,
    remoteParent,
    errorMessage,
    refreshRemote,
    uploadLocalPaths: (paths, targetDirectory) => uploadLocalPaths(paths, targetDirectory),
    t,
  });

const { moveDragActive, moveDropTargetPath, onMoveMouseDown, suppressMoveClick, tableBodyRef } =
  useSftpMoveDrag({
    props,
    remotePath,
    remoteParent,
    remoteFiles,
    remoteFileByName,
    remoteFileMapForPath,
    refreshCurrentDirectoryIncremental,
    selectedNames,
    inlineEdit,
    loading,
    errorMessage,
    refreshRemote,
    closeContextMenu,
    requestRenameConflictAction,
    t,
  });

const {
  activeTransfers,
  cancelTransfer,
  chooseAndUpload,
  clearCompletedTransfers,
  completedTransfers,
  downloadEntry: downloadEntryViaTransfers,
  pauseTransfer,
  removeTransfer,
  resumeTransfer,
  startDownload,
  transfers,
  uploadLocalPaths,
} = useSftpTransfers({
  props,
  remotePath,
  clearDropTarget: () => {
    dropTargetPath.value = "";
  },
  remoteFileByName,
  refreshCurrentDirectoryIncremental,
  requestUploadConflictAction,
  selectedEntry,
  selectedNames,
  t,
});

const {
  activeEditorPath,
  activeEditorTab,
  closeEditor,
  discardEditorChanges,
  editorTabs,
  openEditor,
  saveEditor,
  updateEditorContent,
} = useSftpEditors({
  props,
  t,
  refreshCurrentDirectoryIncremental,
  requestDirtyAction: requestDirtyEditorAction,
  requestOverwriteAction: requestOverwriteEditorAction,
});

useSftpMotion({
  tableBodyRef,
  queueListRef,
  filteredRemoteFiles,
  loading,
  dragActive,
  moveDragActive,
  transfers,
});

watch(
  [activeTransfers, () => transfers.value.length],
  ([count], [previousCount] = []) => {
    if (count > 0) {
      queueCollapsed.value = false;
      return;
    }

    if (previousCount === undefined) {
      queueCollapsed.value = true;
    }
  },
  { immediate: true },
);

watch(hasEditorTabs, (value) => {
  if (!value) editorMode.value = false;
});

function downloadEntry(entry = selectedEntry.value) {
  return downloadEntryViaTransfers(entry, closeContextMenu);
}

function openEditorFromContext(entry) {
  closeContextMenu();
  editorMode.value = true;
  return openEditor(entry);
}

async function saveEditorAndReturn(tab = activeEditorTab.value) {
  const saved = await saveEditor(tab);
  if (saved) editorMode.value = false;
}

async function returnToFileManager() {
  if (currentEditorDirty.value) {
    const action = await requestDirtyEditorAction({
      kind: "leave",
      tab: activeEditorTab.value,
    });
    if (action === "save") {
      const saved = await saveEditor(activeEditorTab.value);
      if (!saved) return;
    } else if (action === "discard") {
      discardEditorChanges(activeEditorTab.value);
    } else {
      return;
    }
  }
  editorMode.value = false;
}

function openParentDirectory() {
  if (!parentDirectoryEntry.value) return;
  selectedNames.value = new Set();
  closeContextMenu();
  refreshRemote(parentDirectoryEntry.value.path);
}

function setPathInputRef(element) {
  pathInput.value = element;
}

function setInlineEditInputRef(element) {
  inlineEditInput.value = element;
}

function updateInlineEditValue(value) {
  inlineEdit.value = { ...inlineEdit.value, value };
}

function setTableBodyRef(element) {
  tableBodyRef.value = element;
}

function setQueueListRef(element) {
  queueListRef.value = element;
}
</script>

<template>
  <div
    class="sftp-root"
    :class="{
      'is-dragging': dragActive || moveDragActive,
      'sftp-root-editor-mode': editorMode && hasEditorTabs,
      'sftp-root-queue-collapsed': queueCollapsed,
    }"
    @contextmenu="provideContextMenu"
  >
    <TabsRoot
      v-if="editorMode && hasEditorTabs"
      v-model="activeEditorPath"
      class="sftp-editor-panel"
      activation-mode="manual"
    >
      <TabsList
        class="sftp-editor-tabs"
        :aria-label="t('sftp.editor.tabs')"
      >
        <div
          v-for="tab in editorTabs"
          :key="tab.path"
          class="sftp-editor-tab"
          :class="{
            'is-dirty': tab.content !== tab.savedContent,
          }"
        >
          <TabsTrigger
            :value="tab.path"
            class="sftp-editor-tab-activate"
          >
            <span class="sftp-editor-tab-name">{{ tab.name }}</span>
          </TabsTrigger>
          <span
            v-if="tab.content !== tab.savedContent"
            class="sftp-editor-tab-dirty"
            aria-hidden="true"
          />
          <button
            type="button"
            class="sftp-editor-tab-close"
            :aria-label="t('sftp.editor.closeTab', { name: tab.name })"
            @click.stop="closeEditor(tab)"
          >
            <X
              :size="13"
              stroke-width="2"
            />
          </button>
        </div>
      </TabsList>

      <CodeEditor
        v-if="activeEditorTab"
        :back-label="t('sftp.editor.backToFiles')"
        :content="activeEditorTab.content"
        :dirty="activeEditorTab.content !== activeEditorTab.savedContent"
        :error="activeEditorTab.error"
        :font-family="preferences.editorFontFamily"
        :font-size="preferences.editorFontSize"
        :highlight-current-line="preferences.editorHighlightActiveLine"
        :line-wrapping="preferences.editorLineWrapping"
        :loading="activeEditorTab.loading"
        :loading-label="t('sftp.editor.loading')"
        :path="activeEditorTab.path"
        :readonly="false"
        :resolved-theme="resolvedEditorTheme"
        :save-label="t('actions.save')"
        :saving="activeEditorTab.saving"
        :tab-size="preferences.editorTabSize"
        :title="activeEditorTab.name"
        @back="returnToFileManager"
        @font-size-change="(size) => (preferences.editorFontSize = size)"
        @save="saveEditor(activeEditorTab)"
        @save-and-back="saveEditorAndReturn(activeEditorTab)"
        @update:content="updateEditorContent(activeEditorTab.path, $event)"
      />
    </TabsRoot>

    <template v-else>
      <SftpToolbar
        :can-download="canDownload"
        :download-label="t('sftp.download')"
        :endpoint-label="endpointLabel"
        :refresh-label="t('sftp.refresh')"
        :title="t('sftp.title')"
        :upload-label="t('sftp.upload')"
        @download="startDownload"
        @refresh="refreshRemote()"
        @upload="chooseAndUpload"
      />

      <SftpPathBar
        v-model:editable-path="editablePath"
        v-model:remote-query="remoteQuery"
        :edit-path-label="t('sftp.editPath')"
        :editing-path="editingPath"
        :path-crumbs="pathCrumbs"
        :path-loading="pathLoading"
        :remote-parent="remoteParent"
        :remote-path-label="t('sftp.remotePath')"
        :search-label="t('sftp.search')"
        :set-path-input-ref="setPathInputRef"
        @cancel-path-edit="cancelPathEdit"
        @edit-path="startPathEdit"
        @navigate="refreshRemote"
        @navigate-parent="refreshRemote(remoteParent)"
        @submit-path-edit="submitPathEdit"
      />

      <div
        v-if="errorMessage"
        class="sftp-inline-error"
      >
        <AlertCircle
          :size="14"
          stroke-width="1.9"
        />
        <span class="min-w-0 truncate">{{ errorMessage }}</span>
      </div>

      <SftpFileTable
        :cancel-inline-edit="cancelInlineEdit"
        :commit-inline-edit="commitInlineEdit"
        :creating-entry="creatingEntry"
        :creating-folder="creatingFolder"
        :drop-target-path="dropTargetPath"
        :file-type-label="fileTypeLabel"
        :filtered-remote-files="filteredRemoteFiles"
        :format-modified="formatModified"
        :inline-edit="inlineEdit"
        :is-editing-entry="isEditingEntry"
        :labels="fileTableLabels"
        :loading="loading"
        :move-drag-active="dragActive || moveDragActive"
        :move-drop-target-path="moveDropTargetPath"
        :parent-directory-entry="parentDirectoryEntry"
        :selected-names="selectedNames"
        :set-inline-edit-input-ref="setInlineEditInputRef"
        :set-table-body-ref="setTableBodyRef"
        @clear-selection="clearSelectionOnBlankClick"
        @dom-drag-leave="onTableDomDragLeave"
        @dom-drag-over="onTableDomDragOver"
        @dom-drop="onTableDomDrop"
        @move-mouse-down="onMoveMouseDown"
        @open-entry="openEntry"
        @open-parent="openParentDirectory"
        @select-entry="selectEntry"
        @start-rename-entry="startRenameEntry"
        @suppress-move-click="suppressMoveClick"
        @update-inline-edit-value="updateInlineEditValue"
      />

      <SftpTransferQueue
        v-model:is-collapsed="queueCollapsed"
        :active-transfers="activeTransfers"
        :clear-completed-label="t('sftp.clearCompleted')"
        :collapse-label="t('common.collapse')"
        :completed-label="t('sftp.completed')"
        :completed-transfers="completedTransfers"
        :expand-label="t('common.expand')"
        :no-transfers-label="t('sftp.noTransfers')"
        :running-label="t('sftp.active')"
        :title="t('sftp.transferQueue')"
        :transfers="transfers"
        :cancel-label="t('sftp.cancelTransfer')"
        :pause-label="t('sftp.pauseTransfer')"
        :resume-label="t('sftp.resumeTransfer')"
        :set-queue-list-ref="setQueueListRef"
        @cancel="cancelTransfer"
        @clear-completed="clearCompletedTransfers"
        @pause="pauseTransfer"
        @remove="removeTransfer"
        @resume="resumeTransfer"
      />
    </template>

    <ConfirmDialog
      :open="deleteDialog.open"
      tone="danger"
      :loading="deleteDialog.deleting"
      :title="deleteDialogTitle"
      :description="deleteDialogDescription"
      :confirm-text="t('sftp.deleteAction')"
      @update:open="closeDeleteDialogOnDismiss"
      @confirm="confirmDeleteEntries"
    >
      <template v-if="deleteDialog.entries.length">
        <div class="sftp-delete-summary">
          <span class="sftp-delete-summary-item sftp-delete-summary-file">
            <File
              :size="14"
              stroke-width="1.9"
              aria-hidden="true"
            />
            {{ t("sftp.deleteFileCount", { count: deleteEntriesSummary.files }) }}
          </span>
          <span class="sftp-delete-summary-item sftp-delete-summary-folder">
            <Folder
              :size="14"
              stroke-width="1.9"
              aria-hidden="true"
            />
            {{ t("sftp.deleteFolderCount", { count: deleteEntriesSummary.folders }) }}
          </span>
        </div>
        <ul class="sftp-delete-list">
          <li
            v-for="entry in deleteDialog.entries.slice(0, 6)"
            :key="entry.path"
            class="sftp-delete-item"
            :class="entry.kind === 'dir' ? 'is-folder' : 'is-file'"
          >
            <span class="sftp-delete-kind">
              <Folder
                v-if="entry.kind === 'dir'"
                :size="14"
                stroke-width="1.9"
                aria-hidden="true"
              />
              <File
                v-else
                :size="14"
                stroke-width="1.9"
                aria-hidden="true"
              />
              {{ entry.kind === "dir" ? t("sftp.folder") : t("sftp.file") }}
            </span>
            <span class="sftp-delete-name">{{ entry.name }}</span>
          </li>
          <li
            v-if="deleteDialog.entries.length > 6"
            class="sftp-delete-more"
          >
            {{ t("sftp.deleteMoreItems", { count: deleteDialog.entries.length - 6 }) }}
          </li>
        </ul>
      </template>
    </ConfirmDialog>

    <ConfirmDialog
      :open="dirtyEditorDialog.open"
      tone="warning"
      :title="editorConfirmTitle"
      :description="editorConfirmDescription"
      :confirm-text="editorConfirmText"
      :secondary-text="editorConfirmSecondaryText"
      @update:open="onDirtyEditorDialogOpenChange"
      @confirm="resolveDirtyEditorAction(dirtyEditorDialog.kind === 'overwrite' ? true : 'save')"
      @secondary="resolveDirtyEditorAction('discard')"
    />

    <ConfirmDialog
      :open="conflictDialog.open"
      tone="warning"
      :title="conflictDialog.title"
      :description="conflictDialog.description"
      :confirm-text="
        conflictDialog.kind === 'rename'
          ? t('sftp.renameConflictOverwriteAction')
          : t('sftp.uploadOverwriteAction')
      "
      :secondary-text="conflictDialog.kind === 'rename' ? '' : t('sftp.uploadResumeAction')"
      :cancel-text="
        conflictDialog.kind === 'rename' ? t('actions.cancel') : t('sftp.uploadSkipAction')
      "
      @update:open="onConflictDialogOpenChange"
      @confirm="resolveConflictAction('overwrite')"
      @secondary="resolveConflictAction('resume')"
    />
  </div>
</template>
