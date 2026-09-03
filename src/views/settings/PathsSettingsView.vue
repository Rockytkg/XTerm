<script setup>
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { Database, FolderCog, Power } from "@lucide/vue";
import { useToasts } from "../../composables/useToasts";
import { restartApp } from "../../services/appInfo";
import { chooseDirectory, getPathSettings, setPathSettings } from "../../services/pathSettings";
import ConfirmDialog from "../../components/ConfirmDialog.vue";
import { createLogger } from "../../utils/logger";

const logger = createLogger("frontend.settings.paths");

const { t } = useI18n();
const { showToast } = useToasts();
const pathSettings = ref({ installDir: "", dataDir: "", logDir: "" });
const pathSettingsSaving = ref(false);
const pathSettingsError = ref("");
const restartConfirmOpen = ref(false);

const restartDescription = computed(() => t("settings.paths.restartConfirm.description"));

async function loadPathSettings() {
  try {
    pathSettings.value = await getPathSettings();
  } catch (error) {
    pathSettingsError.value = String(error);
    logger.error("path-settings.load.failed", error);
  }
}

async function updatePathSettings() {
  pathSettingsSaving.value = true;
  pathSettingsError.value = "";
  try {
    pathSettings.value = await setPathSettings(pathSettings.value);
    restartConfirmOpen.value = true;
  } catch (error) {
    pathSettingsError.value = String(error);
    showToast({
      type: "error",
      title: t("notifications.pathSettingsSaveFailed"),
      message: String(error),
    });
    logger.error("path-settings.update.failed", error);
  } finally {
    pathSettingsSaving.value = false;
  }
}

async function browsePathSetting(key) {
  try {
    const selected = await chooseDirectory(
      pathSettings.value[key],
      t("settings.paths.chooseDirectoryTitle"),
    );
    if (!selected) return;
    pathSettings.value = { ...pathSettings.value, [key]: selected };
    await updatePathSettings();
  } catch (error) {
    pathSettingsError.value = String(error);
    logger.error("directory.choose.failed", error);
  }
}

async function confirmRestartApp() {
  try {
    await restartApp();
  } catch (error) {
    logger.error("app.restart.failed", error);
    showToast({
      type: "error",
      title: t("notifications.appRestartFailed"),
      message: String(error),
    });
  }
}

loadPathSettings();
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <FolderCog
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.paths.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.paths.hint") }}
        </p>
      </div>
    </div>
    <div class="settings-fields">
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.paths.dataDirectory") }}</span>
        <div class="settings-path-row">
          <input
            v-model="pathSettings.dataDir"
            class="ui-input"
            @change="updatePathSettings"
          >
          <button
            type="button"
            class="ui-button-secondary settings-path-button"
            @click="browsePathSetting('dataDir')"
          >
            <Database
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.paths.browseDirectory") }}
          </button>
        </div>
        <span class="settings-hint">{{ t("settings.paths.dataDirectoryHint") }}</span>
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.paths.logDirectory") }}</span>
        <div class="settings-path-row">
          <input
            v-model="pathSettings.logDir"
            class="ui-input"
            @change="updatePathSettings"
          >
          <button
            type="button"
            class="ui-button-secondary settings-path-button"
            @click="browsePathSetting('logDir')"
          >
            <Database
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.paths.browseDirectory") }}
          </button>
        </div>
        <span class="settings-hint">{{ t("settings.paths.logDirectoryHint") }}</span>
      </div>
      <div
        v-if="pathSettingsError || pathSettingsSaving"
        class="settings-field"
      >
        <span class="settings-hint">{{
          pathSettingsSaving ? t("settings.paths.preparingDirectories") : pathSettingsError
        }}</span>
      </div>
    </div>
    <ConfirmDialog
      v-model:open="restartConfirmOpen"
      tone="info"
      :title="t('settings.paths.restartConfirm.title')"
      :description="restartDescription"
      :confirm-text="t('settings.paths.restartConfirm.restartNow')"
      :cancel-text="t('settings.paths.restartConfirm.later')"
      :confirm-icon="Power"
      @confirm="confirmRestartApp"
    />
  </section>
</template>
