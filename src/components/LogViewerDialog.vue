<script setup>
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { Copy, FolderOpen, RefreshCw, ScrollText, Trash2 } from "@lucide/vue";
import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import UiSelect from "./UiSelect.vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import { listLogFiles, openLogDir, pruneLogFiles, readLogTail } from "../services/logging";
import { useToasts } from "../composables/useToasts";
import { createLogger } from "../utils/logger";
import { formatBytes } from "../utils/formatBytes";

const props = defineProps({
  open: { type: Boolean, default: false },
});
const emit = defineEmits(["update:open"]);

const { t } = useI18n();
const { showToast } = useToasts();
const logger = createLogger("frontend.settings.logs");

const files = ref([]);
const selectedFile = ref("");
const content = ref("");
const loading = ref(false);
const pruning = ref(false);
const pruneConfirmOpen = ref(false);

const fileOptions = computed(() =>
  files.value.map((file) => ({
    label: `${file.name} (${formatBytes(file.sizeBytes)})`,
    value: file.name,
  })),
);

const selectedFileMeta = computed(
  () => files.value.find((file) => file.name === selectedFile.value) || null,
);

function setOpen(value) {
  emit("update:open", value);
}

async function loadTail(name) {
  if (!name) {
    content.value = "";
    return;
  }
  loading.value = true;
  try {
    content.value = await readLogTail(name);
  } catch (error) {
    logger.error("log-file.tail.failed", error);
    content.value = "";
    showToast({ type: "error", title: t("notifications.logLoadFailed"), message: String(error) });
  } finally {
    loading.value = false;
  }
}

async function refreshFiles() {
  try {
    files.value = await listLogFiles();
    const stillThere = files.value.some((file) => file.name === selectedFile.value);
    if (!stillThere) {
      selectedFile.value = files.value[0]?.name || "";
    }
    await loadTail(selectedFile.value);
  } catch (error) {
    logger.error("log-files.list.failed", error);
    showToast({ type: "error", title: t("notifications.logLoadFailed"), message: String(error) });
  }
}

function selectFile(name) {
  selectedFile.value = name;
  void loadTail(name);
}

async function copyContent() {
  if (!content.value) return;
  try {
    await writeClipboardText(content.value);
    showToast({ type: "success", title: t("notifications.logCopied") });
  } catch (error) {
    logger.error("log-file.copy.failed", error);
    showToast({ type: "error", title: t("notifications.logCopyFailed"), message: String(error) });
  }
}

async function openDirectory() {
  try {
    await openLogDir();
  } catch (error) {
    logger.error("log-dir.open.failed", error);
    showToast({
      type: "error",
      title: t("notifications.logDirOpenFailed"),
      message: String(error),
    });
  }
}

async function pruneOldLogs() {
  pruneConfirmOpen.value = false;
  pruning.value = true;
  try {
    const removed = await pruneLogFiles();
    showToast({
      type: "success",
      title: t("notifications.logsPruned"),
      message: t("notifications.logsPrunedCount", { count: removed }),
    });
    await refreshFiles();
  } catch (error) {
    logger.error("log-files.prune.failed", error);
    showToast({ type: "error", title: t("notifications.logPruneFailed"), message: String(error) });
  } finally {
    pruning.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void refreshFiles();
  },
);
</script>

<template>
  <DialogRoot
    :open="open"
    @update:open="setOpen"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent class="dialog-content conn-dialog focus:outline-none">
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <ScrollText
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ t("settings.general.logViewer.title") }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ t("settings.general.logViewer.description") }}
            </DialogDescription>
          </div>
        </header>

        <div class="conn-dialog-body flex flex-col gap-[10px] min-h-0">
          <div class="flex items-center gap-[8px]">
            <UiSelect
              :model-value="selectedFile"
              :options="fileOptions"
              class="flex-1"
              @update:model-value="selectFile"
            />
            <button
              type="button"
              class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
              :disabled="loading || !selectedFile"
              @click="loadTail(selectedFile)"
            >
              <RefreshCw
                :size="13"
                stroke-width="1.8"
              />
              {{ t("settings.general.logViewer.refresh") }}
            </button>
          </div>
          <div
            v-if="selectedFileMeta?.modifiedAt"
            class="text-[0.7857em] text-[var(--text-secondary)]"
          >
            {{ selectedFileMeta.modifiedAt }} · {{ formatBytes(selectedFileMeta.sizeBytes) }}
          </div>
          <div
            v-if="!files.length"
            class="ui-empty-state px-[16px] py-[28px] text-[0.8571em]"
          >
            <p>{{ t("settings.general.logViewer.empty") }}</p>
          </div>
          <pre
            v-else
            class="flex-1 min-h-[240px] max-h-[46vh] overflow-auto m-0 px-[10px] py-[8px] rounded-[6px] bg-[var(--bg-secondary)] font-mono text-[11px] leading-[1.6] whitespace-pre-wrap break-all select-text"
          >{{ content || t("settings.general.logViewer.emptyContent") }}</pre>
        </div>

        <footer class="conn-dialog-footer">
          <button
            type="button"
            class="ui-button-secondary flex items-center gap-[6px]"
            :disabled="pruning"
            @click="pruneConfirmOpen = true"
          >
            <Trash2
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.general.logViewer.prune") }}
          </button>
          <button
            type="button"
            class="ui-button-secondary flex items-center gap-[6px]"
            @click="openDirectory"
          >
            <FolderOpen
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.general.logViewer.openDir") }}
          </button>
          <button
            type="button"
            class="ui-button-secondary flex items-center gap-[6px]"
            :disabled="!content"
            @click="copyContent"
          >
            <Copy
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.general.logViewer.copy") }}
          </button>
          <button
            type="button"
            class="ui-button-primary"
            @click="setOpen(false)"
          >
            {{ t("actions.close") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>

  <ConfirmDialog
    v-model:open="pruneConfirmOpen"
    tone="warning"
    :title="t('settings.general.logViewer.pruneConfirmTitle')"
    :description="t('settings.general.logViewer.pruneConfirmDescription')"
    :confirm-text="t('settings.general.logViewer.prune')"
    :loading="pruning"
    @confirm="pruneOldLogs"
  />
</template>
