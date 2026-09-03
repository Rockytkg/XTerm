<script setup>
import "../styles/scripts-view.scss";

import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { onBeforeRouteLeave } from "vue-router";
import { storeToRefs } from "pinia";
import { CirclePlus, Download, FileCode, Pencil, RefreshCw, Trash2, Upload } from "@lucide/vue";
import AppTooltip from "../components/AppTooltip.vue";
import CodeEditor from "../components/CodeEditor.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import MarqueeText from "../components/MarqueeText.vue";
import ScriptCreateDialog from "../components/ScriptCreateDialog.vue";
import { useToasts } from "../composables/useToasts";
import { openExternalUrl } from "../services/appInfo";
import { exportScriptFile, pickScriptFile } from "../services/scripting/scriptFileLoader";
import { useScriptsStore } from "../stores/scriptsStore";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { resolveEditorTheme } from "../utils/editorTheme";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.scripts.view");

const { t } = useI18n();
const { showToast } = useToasts();
const scriptsStore = useScriptsStore();
const workspace = useWorkspaceStore();
const { scripts, updateChecking } = storeToRefs(scriptsStore);
const { preferences, resolvedTheme } = storeToRefs(workspace);

const editingId = ref("");
const editorContent = ref("");
const savedEditorContent = ref("");
const editorSaving = ref(false);
const dirtyEditorDialogOpen = ref(false);
const createDialogOpen = ref(false);
const deleteTarget = ref(null);
let resolveDirtyEditorAction = null;
let dirtyEditorActionPromise = null;
// Ctrl+滚轮缩放（CodeEditor 内部处理），初值跟随全局编辑器字号偏好。
const scriptFontSize = ref(preferences.value.editorFontSize);

const editingScript = computed(
  () => scripts.value.find((script) => script.id === editingId.value) || null,
);
const resolvedEditorTheme = computed(() =>
  resolveEditorTheme(preferences.value?.editorThemeMode, resolvedTheme.value),
);
const editorDirty = computed(
  () => !!editingScript.value && editorContent.value !== savedEditorContent.value,
);

function formatTime(timestamp) {
  if (!timestamp) return "-";
  const date = new Date(timestamp);
  const pad = (value) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function openAuthorHomepage(script) {
  if (script.homepage) void openExternalUrl(script.homepage);
}

function openEditor(id) {
  const script = scriptsStore.getScript(id);
  if (!script) return;
  editingId.value = id;
  editorContent.value = script.code;
  savedEditorContent.value = script.code;
}

function closeEditor() {
  editingId.value = "";
  editorContent.value = "";
  savedEditorContent.value = "";
}

function onScriptCreated(script) {
  openEditor(script.id);
}

function onEditorContent(code) {
  editorContent.value = code;
}

function onFormatError(error) {
  showToast({
    type: "error",
    title: t("scripts.formatFailed"),
    message: String(error?.message || error),
  });
}

function onFormatted(changed) {
  showToast({
    type: changed ? "success" : "info",
    title: t(changed ? "scripts.formatDone" : "scripts.formatNoChanges"),
  });
}

async function saveScriptsNow() {
  if (!editingScript.value || editorSaving.value) return false;
  editorSaving.value = true;
  try {
    scriptsStore.updateScript(editingScript.value.id, { code: editorContent.value });
    await scriptsStore.persistNow();
    savedEditorContent.value = editorContent.value;
    showToast({ type: "success", title: t("notifications.scriptSaved") });
    return true;
  } catch (error) {
    logger.error("scripts.save.failed", error);
    showToast({
      type: "error",
      title: t("notifications.scriptSaveFailed"),
      message: String(error?.message || error),
    });
    return false;
  } finally {
    editorSaving.value = false;
  }
}

async function saveScriptsAndBack() {
  if (await saveScriptsNow()) closeEditor();
}

function requestDirtyAction() {
  if (!editorDirty.value) return Promise.resolve("discard");
  if (dirtyEditorActionPromise) return dirtyEditorActionPromise;
  dirtyEditorDialogOpen.value = true;
  dirtyEditorActionPromise = new Promise((resolve) => {
    resolveDirtyEditorAction = resolve;
  });
  return dirtyEditorActionPromise;
}

function finishDirtyAction(action) {
  dirtyEditorDialogOpen.value = false;
  const resolve = resolveDirtyEditorAction;
  resolveDirtyEditorAction = null;
  dirtyEditorActionPromise = null;
  resolve?.(action);
}

function onDirtyDialogOpenChange(open) {
  if (!open && dirtyEditorDialogOpen.value) finishDirtyAction("cancel");
}

async function requestCloseEditor() {
  if (!editorDirty.value) {
    closeEditor();
    return;
  }
  const action = await requestDirtyAction();
  if (action === "save") {
    if (await saveScriptsNow()) closeEditor();
  } else if (action === "discard") {
    closeEditor();
  }
}

function fileDialogLabels(titleKey) {
  return {
    title: t(titleKey),
    jsFilesLabel: t("scripts.fileDialog.jsFiles"),
    allFilesLabel: t("scripts.fileDialog.allFiles"),
  };
}

async function importScript() {
  try {
    const file = await pickScriptFile(fileDialogLabels("scripts.fileDialog.pickTitle"));
    if (!file) return;
    scriptsStore.importScript(file.name, file.code);
  } catch (error) {
    logger.error("scripts.import.failed", error);
    showToast({
      type: "error",
      title: t("notifications.scriptFileReadFailed"),
      message: String(error),
    });
  }
}

async function exportScript(script) {
  try {
    const fileName = `${(script.name || "script").replace(/[\\/:*?"<>|]/g, "_")}.js`;
    const path = await exportScriptFile(
      fileName,
      script.code,
      fileDialogLabels("scripts.fileDialog.exportTitle"),
    );
    if (!path) return;
    showToast({
      type: "success",
      title: t("notifications.scriptExported"),
      message: path,
    });
  } catch (error) {
    logger.error("scripts.export.failed", error);
    showToast({
      type: "error",
      title: t("notifications.scriptExportFailed"),
      message: String(error),
    });
  }
}

async function checkUpdates() {
  const result = await scriptsStore.checkAllUpdates();
  if (result.errors) {
    showToast({ type: "warning", title: t("notifications.scriptUpdateCheckFailed") });
    return;
  }
  showToast({
    type: result.available ? "success" : "info",
    title: t("notifications.scriptUpdateCheckDone", { count: result.available }),
  });
}

async function applyUpdate(script) {
  try {
    await scriptsStore.applyScriptUpdate(script.id);
    showToast({
      type: "success",
      title: t("notifications.scriptUpdateApplied", { name: script.name }),
    });
  } catch (error) {
    logger.error("scripts.update.apply.failed", error);
    showToast({
      type: "error",
      title: t("notifications.scriptUpdateFailed"),
      message: String(error),
    });
  }
}

function requestDelete(script) {
  deleteTarget.value = script;
}

function confirmDelete() {
  if (!deleteTarget.value) return;
  scriptsStore.removeScript(deleteTarget.value.id);
  deleteTarget.value = null;
}

onMounted(() => {
  if (!scriptsStore.loaded) void scriptsStore.loadScripts();
});

onBeforeRouteLeave(async () => {
  if (!editorDirty.value) return true;
  const action = await requestDirtyAction();
  if (action === "save") return await saveScriptsNow();
  return action === "discard";
});
</script>

<template>
  <div class="scripts-root">
    <template v-if="!editingScript">
      <div class="ui-page-header">
        <div class="ui-page-header-main">
          <FileCode
            :size="18"
            stroke-width="1.6"
            class="text-accent"
          />
          <div>
            <h2 class="ui-page-title">
              {{ t("scripts.title") }}
            </h2>
            <p class="ui-page-desc">
              {{ t("scripts.description") }}
            </p>
          </div>
        </div>

        <div class="scripts-actions">
          <button
            type="button"
            class="credential-toolbar-button"
            :disabled="updateChecking"
            @click="checkUpdates"
          >
            <RefreshCw
              :size="14"
              stroke-width="1.8"
              :class="{ 'scripts-spin': updateChecking }"
            />
            <span>{{ t("scripts.checkUpdates") }}</span>
          </button>
          <button
            type="button"
            class="credential-toolbar-button"
            @click="importScript"
          >
            <Upload
              :size="14"
              stroke-width="1.8"
            />
            <span>{{ t("scripts.import") }}</span>
          </button>
          <button
            type="button"
            class="credential-toolbar-button credential-toolbar-primary"
            @click="createDialogOpen = true"
          >
            <CirclePlus
              :size="14"
              stroke-width="2"
            />
            <span>{{ t("scripts.add") }}</span>
          </button>
        </div>
      </div>

      <div class="scripts-directory">
        <div
          v-if="!scripts.length"
          class="ui-empty-state px-[24px] py-[60px] text-[0.9286em]"
        >
          <FileCode
            :size="32"
            stroke-width="1.2"
            class="text-text-tertiary mb-[12px]"
          />
          <p>{{ t("scripts.empty") }}</p>
        </div>

        <div
          v-else
          class="scripts-table-wrap"
        >
          <table class="scripts-table">
            <thead>
              <tr>
                <th>{{ t("scripts.columns.name") }}</th>
                <th>{{ t("scripts.columns.description") }}</th>
                <th>{{ t("scripts.columns.author") }}</th>
                <th>{{ t("scripts.columns.version") }}</th>
                <th>{{ t("scripts.columns.updated") }}</th>
                <th class="scripts-actions-column">
                  {{ t("scripts.columns.actions") }}
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="script in scripts"
                :key="script.id"
                class="scripts-table-row"
                @dblclick="openEditor(script.id)"
              >
                <td class="scripts-name-cell">
                  <button
                    type="button"
                    class="scripts-name-button"
                    @click="openEditor(script.id)"
                  >
                    <FileCode
                      :size="15"
                      stroke-width="1.7"
                    />
                    <MarqueeText :text="script.name || t('scripts.untitled')" />
                  </button>
                </td>
                <td>
                  <MarqueeText
                    class="scripts-description-cell"
                    :text="script.description || t('scripts.noDescription')"
                  />
                </td>
                <td>
                  <button
                    v-if="script.homepage"
                    type="button"
                    class="scripts-author-link"
                    @click="openAuthorHomepage(script)"
                  >
                    <MarqueeText :text="script.author || script.homepage" />
                  </button>
                  <MarqueeText
                    v-else
                    :text="script.author || '-'"
                  />
                </td>
                <td>
                  <div class="scripts-version-cell">
                    <span>{{ script.version ? `v${script.version}` : "-" }}</span>
                    <span
                      v-if="script.updateAvailableVersion"
                      class="scripts-update-badge"
                    >{{
                      t("scripts.updateAvailable", { version: script.updateAvailableVersion })
                    }}</span>
                  </div>
                </td>
                <td class="scripts-time-cell">
                  {{ formatTime(script.updatedAt) }}
                </td>
                <td>
                  <div class="scripts-row-actions">
                    <AppTooltip
                      v-if="script.updateAvailableVersion"
                      :content="t('scripts.applyUpdate')"
                      side="top"
                    >
                      <button
                        type="button"
                        class="ui-row-action"
                        :aria-label="t('scripts.applyUpdate')"
                        @click="applyUpdate(script)"
                      >
                        <Download
                          :size="13"
                          stroke-width="1.8"
                        />
                      </button>
                    </AppTooltip>
                    <AppTooltip
                      :content="t('actions.edit')"
                      side="top"
                    >
                      <button
                        type="button"
                        class="ui-row-action"
                        :aria-label="t('actions.edit')"
                        @click="openEditor(script.id)"
                      >
                        <Pencil
                          :size="13"
                          stroke-width="1.8"
                        />
                      </button>
                    </AppTooltip>
                    <AppTooltip
                      :content="t('scripts.export')"
                      side="top"
                    >
                      <button
                        type="button"
                        class="ui-row-action"
                        :aria-label="t('scripts.export')"
                        @click="exportScript(script)"
                      >
                        <Upload
                          class="scripts-export-icon"
                          :size="13"
                          stroke-width="1.8"
                        />
                      </button>
                    </AppTooltip>
                    <AppTooltip
                      :content="t('actions.delete')"
                      side="top"
                    >
                      <button
                        type="button"
                        class="ui-row-action ui-row-action-danger"
                        :aria-label="t('actions.delete')"
                        @click="requestDelete(script)"
                      >
                        <Trash2
                          :size="13"
                          stroke-width="1.8"
                        />
                      </button>
                    </AppTooltip>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </template>

    <CodeEditor
      v-else
      :back-label="t('scripts.back')"
      :content="editorContent"
      :dirty="editorDirty"
      :font-family="preferences.editorFontFamily"
      :font-size="scriptFontSize"
      :format-label="t('scripts.format')"
      formatting-enabled
      :highlight-current-line="preferences.editorHighlightActiveLine"
      :line-wrapping="preferences.editorLineWrapping"
      :loading-label="t('sftp.editor.loading')"
      :path="`${editingScript.name || 'script'}.js`"
      :resolved-theme="resolvedEditorTheme"
      :save-label="t('actions.save')"
      :saving="editorSaving"
      :tab-size="preferences.editorTabSize"
      :title="editingScript.name || t('scripts.untitled')"
      @back="requestCloseEditor"
      @font-size-change="(size) => (scriptFontSize = size)"
      @format-error="onFormatError"
      @formatted="onFormatted"
      @save="saveScriptsNow"
      @save-and-back="saveScriptsAndBack"
      @update:content="onEditorContent"
    />

    <ScriptCreateDialog
      v-model:open="createDialogOpen"
      @created="onScriptCreated"
    />

    <ConfirmDialog
      :open="dirtyEditorDialogOpen"
      tone="warning"
      :title="t('scripts.unsaved.title')"
      :description="
        t('scripts.unsaved.description', { name: editingScript?.name || t('scripts.untitled') })
      "
      :confirm-text="t('actions.save')"
      :secondary-text="t('scripts.unsaved.discard')"
      @update:open="onDirtyDialogOpenChange"
      @confirm="finishDirtyAction('save')"
      @secondary="finishDirtyAction('discard')"
    />

    <ConfirmDialog
      :open="!!deleteTarget"
      tone="danger"
      :title="t('scripts.deleteConfirm.title')"
      :description="
        t('scripts.deleteConfirm.description', {
          name: deleteTarget?.name || t('scripts.untitled'),
        })
      "
      :confirm-text="t('actions.delete')"
      :confirm-icon="Trash2"
      @update:open="(open) => !open && (deleteTarget = null)"
      @confirm="confirmDelete"
    />
  </div>
</template>
