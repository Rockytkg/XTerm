<script setup>
import { computed, ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Bug, ExternalLink, GitBranch as Github, Info, RefreshCw } from "@lucide/vue";
import { checkForUpdates, getAppMetadata, openExternalUrl } from "../../services/appInfo";
import { runManualUpdateCheck } from "../../composables/useUpdateChecker";
import { showToast } from "../../composables/useToasts";
import { createLogger } from "../../utils/logger";

const logger = createLogger("frontend.settings.about");

const { t } = useI18n();

const metadata = ref(null);
const updateStatus = ref(null);
const updateError = ref(false);
const checkingUpdates = ref(false);

onMounted(async () => {
  metadata.value = await getAppMetadata();
  await handleCheckUpdates(false);
});

const updateStatusText = computed(() => {
  if (checkingUpdates.value) return t("settings.about.update.checking");
  if (updateError.value) return t("settings.about.update.failed");
  if (!updateStatus.value) return t("settings.about.update.idle");
  if (updateStatus.value.updateAvailable) {
    return t("settings.about.update.available", { version: updateStatus.value.latestVersion });
  }
  return t("settings.about.update.current");
});

async function handleCheckUpdates(notify = true) {
  checkingUpdates.value = true;
  updateError.value = false;
  try {
    // 手动检测命中更新时由全局模态框承接提示，无需再弹 toast
    updateStatus.value = notify ? await runManualUpdateCheck() : await checkForUpdates();
    if (notify && !updateStatus.value.updateAvailable) {
      showToast({ type: "success", title: t("settings.about.update.current") });
    }
  } catch (e) {
    logger.error("updates.check.failed", e);
    updateError.value = true;
    if (notify) showToast({ type: "error", title: t("settings.about.update.failed") });
  } finally {
    checkingUpdates.value = false;
  }
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <Info
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.about.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.about.description") }}
        </p>
      </div>
    </div>
    <div
      v-if="metadata"
      class="settings-fields"
    >
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.about.version") }}</span>
          <span class="settings-hint">
            <span>v{{ metadata.version || "0.0.0" }}</span>
            <span class="settings-inline-divider">/</span>
            <span>{{ updateStatusText }}</span>
          </span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          :disabled="checkingUpdates"
          @click="handleCheckUpdates()"
        >
          <RefreshCw
            :size="13"
            stroke-width="1.8"
            :class="{ 'animate-spin': checkingUpdates }"
          />
          {{ t("settings.about.update.action") }}
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.about.author") }}</span>
          <span class="settings-hint">{{ metadata.author }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openExternalUrl(metadata.author_url)"
        >
          <Github
            :size="13"
            stroke-width="1.8"
          />
          GitHub
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.about.license") }}</span>
          <span class="settings-hint">{{ metadata.license }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openExternalUrl(metadata.license_url)"
        >
          <ExternalLink
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.about.open") }}
        </button>
      </div>
      <div
        v-if="updateStatus?.releaseUrl"
        class="settings-toggle"
      >
        <div>
          <span class="settings-label">{{ t("settings.about.update.release") }}</span>
          <span class="settings-hint">{{
            updateStatus.releaseName || `v${updateStatus.latestVersion}`
          }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openExternalUrl(updateStatus.releaseUrl)"
        >
          <ExternalLink
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.about.open") }}
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.about.repository") }}</span>
          <span class="settings-hint">{{ metadata.repository_url }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openExternalUrl(metadata.repository_url)"
        >
          <Github
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.about.open") }}
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.about.feedback") }}</span>
          <span class="settings-hint">{{ metadata.issues_url }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openExternalUrl(metadata.issues_url)"
        >
          <Bug
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.about.feedbackAction") }}
        </button>
      </div>
    </div>
  </section>
</template>
