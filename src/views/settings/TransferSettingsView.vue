<script setup>
import { UploadCloud } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { computed, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import UiSwitch from "../../components/UiSwitch.vue";
import { useToasts } from "../../composables/useToasts";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { createLogger } from "../../utils/logger";

const { t } = useI18n();
const { showToast } = useToasts();
const logger = createLogger("frontend.settings.transfer");
const workspace = useWorkspaceStore();
const { preferences, fileServiceConfig } = storeToRefs(workspace);
const TRZSZ_DOCS_URL = "https://trzsz.github.io/cn/";
const savingFileServiceCredentials = ref(false);
const fileServiceCredentialDirty = ref(false);
// password 不回显：draft 只承载“新口令”，是否已设置看 fileServiceConfig.passwordSet。
const fileServiceCredentialDraft = reactive({ username: "", password: "" });

const fileServicePasswordPlaceholder = computed(() =>
  fileServiceConfig.value.passwordSet
    ? t("settings.transfer.fileServicePasswordSetPlaceholder")
    : t("settings.transfer.fileServicePasswordUnsetPlaceholder"),
);

watch(
  () => fileServiceConfig.value.username,
  (username) => {
    if (savingFileServiceCredentials.value || fileServiceCredentialDirty.value) return;
    fileServiceCredentialDraft.username = username || "";
  },
  { immediate: true },
);

function normalizeNumberPreference(key, fallback, min, max) {
  const value = Math.round(Number(preferences.value[key]));
  preferences.value[key] = Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

function revertFileServiceCredentialDraft() {
  fileServiceCredentialDraft.username = fileServiceConfig.value.username || "";
  fileServiceCredentialDraft.password = "";
  fileServiceCredentialDirty.value = false;
}

async function saveFileServiceUsername() {
  if (savingFileServiceCredentials.value) return;
  savingFileServiceCredentials.value = true;
  try {
    const config = await workspace.updateFileServiceUsername(fileServiceCredentialDraft.username);
    fileServiceCredentialDraft.username = config.username || "";
    fileServiceCredentialDirty.value = false;
  } catch (error) {
    logger.error("file_service.credentials.update.failed", error);
    revertFileServiceCredentialDraft();
    showToast({
      type: "error",
      title: t("notifications.fileServiceOperationFailed"),
      message: String(error),
    });
  } finally {
    savingFileServiceCredentials.value = false;
  }
}

async function saveFileServicePassword() {
  if (savingFileServiceCredentials.value) return;
  savingFileServiceCredentials.value = true;
  try {
    // 空串提交 = 重置为默认口令（契约：file_service_set_password("")）。
    await workspace.updateFileServicePassword(fileServiceCredentialDraft.password);
    fileServiceCredentialDraft.password = "";
    fileServiceCredentialDirty.value = false;
  } catch (error) {
    logger.error("file_service.password.update.failed", error);
    revertFileServiceCredentialDraft();
    showToast({
      type: "error",
      title: t("notifications.fileServiceOperationFailed"),
      message: String(error),
    });
  } finally {
    savingFileServiceCredentials.value = false;
  }
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <UploadCloud
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.transfer.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.transfer.hint") }}
        </p>
      </div>
    </div>
    <div class="settings-fields">
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.trzsz") }}</span>
          <span class="settings-hint">
            {{ t("settings.transfer.trzszHint") }}
            <a
              :href="TRZSZ_DOCS_URL"
              target="_blank"
              rel="noopener noreferrer"
              class="text-accent underline underline-offset-2"
            >
              {{ t("settings.transfer.trzszLearnMore") }}
            </a>
          </span>
        </div>
        <UiSwitch v-model="preferences.terminalTrzsz" />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.dragUpload") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.dragUploadHint") }}</span>
        </div>
        <UiSwitch
          v-model="preferences.transferDragUpload"
          :disabled="!preferences.terminalTrzsz"
        />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.directoryUpload") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.directoryUploadHint") }}</span>
        </div>
        <UiSwitch
          v-model="preferences.transferDirectoryUpload"
          :disabled="!preferences.terminalTrzsz"
        />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.maxChunkSize") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.maxChunkSizeHint") }}</span>
        </div>
        <input
          v-model.number="preferences.transferMaxChunkSize"
          type="number"
          min="262144"
          max="67108864"
          step="262144"
          class="ui-input ui-input-inline"
          :disabled="!preferences.terminalTrzsz"
          @change="normalizeNumberPreference('transferMaxChunkSize', 10485760, 262144, 67108864)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.dragInitTimeout") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.dragInitTimeoutHint") }}</span>
        </div>
        <input
          v-model.number="preferences.transferDragInitTimeout"
          type="number"
          min="1000"
          max="30000"
          step="500"
          class="ui-input ui-input-inline"
          :disabled="!preferences.terminalTrzsz"
          @change="normalizeNumberPreference('transferDragInitTimeout', 3000, 1000, 30000)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.fileServiceUsername") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.fileServiceUsernameHint") }}</span>
        </div>
        <input
          v-model="fileServiceCredentialDraft.username"
          class="ui-input ui-input-inline"
          autocomplete="off"
          :disabled="savingFileServiceCredentials"
          @input="fileServiceCredentialDirty = true"
          @change="saveFileServiceUsername"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.transfer.fileServicePassword") }}</span>
          <span class="settings-hint">{{ t("settings.transfer.fileServicePasswordHint") }}</span>
        </div>
        <input
          v-model="fileServiceCredentialDraft.password"
          class="ui-input ui-input-inline"
          type="password"
          autocomplete="new-password"
          :placeholder="fileServicePasswordPlaceholder"
          :disabled="savingFileServiceCredentials"
          @input="fileServiceCredentialDirty = true"
          @change="saveFileServicePassword"
        >
      </div>
    </div>
  </section>
</template>
