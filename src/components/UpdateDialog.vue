<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { Download, ExternalLink, PackageOpen } from "@lucide/vue";
import {
  closeUpdateDialog,
  downloadUpdate,
  openUpdateReleasePage,
  useUpdateChecker,
} from "../composables/useUpdateChecker";

const { t } = useI18n();
const { status, dialogOpen } = useUpdateChecker();

const releaseTitle = computed(
  () => status.value?.releaseName || `v${status.value?.latestVersion || ""}`,
);

const publishedAtText = computed(() => {
  const raw = status.value?.publishedAt;
  if (!raw) return "";
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString();
});

const releaseNotes = computed(() => (status.value?.releaseNotes || "").trim());

function setOpen(value) {
  if (!value) closeUpdateDialog();
}

function handleDownload() {
  void downloadUpdate();
  closeUpdateDialog();
}
</script>

<template>
  <DialogRoot
    :open="dialogOpen"
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
            <PackageOpen
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ t("settings.about.update.dialog.title", { version: status?.latestVersion }) }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ releaseTitle }}
              <template v-if="publishedAtText">
                · {{ t("settings.about.update.dialog.publishedAt", { date: publishedAtText }) }}
              </template>
            </DialogDescription>
          </div>
        </header>

        <div class="conn-dialog-body flex flex-col gap-[10px] min-h-0">
          <span class="text-[0.8571em] font-medium text-[var(--text-primary)]">
            {{ t("settings.about.update.dialog.notes") }}
          </span>
          <pre
            class="min-h-[120px] max-h-[46vh] overflow-auto m-0 px-[10px] py-[8px] rounded-[6px] bg-[var(--bg-secondary)] font-mono text-[11px] leading-[1.6] whitespace-pre-wrap break-all select-text"
          >{{ releaseNotes || t("settings.about.update.dialog.noNotes") }}</pre>
        </div>

        <footer class="conn-dialog-footer">
          <button
            type="button"
            class="ui-button-secondary flex items-center gap-[6px]"
            @click="openUpdateReleasePage"
          >
            <ExternalLink
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.about.update.dialog.viewRelease") }}
          </button>
          <button
            type="button"
            class="ui-button-secondary"
            @click="closeUpdateDialog"
          >
            {{ t("settings.about.update.dialog.later") }}
          </button>
          <button
            type="button"
            class="ui-button-primary flex items-center gap-[6px]"
            @click="handleDownload"
          >
            <Download
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.about.update.dialog.download") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
