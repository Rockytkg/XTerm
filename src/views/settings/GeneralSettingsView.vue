<script setup>
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Bug, Languages, ScrollText } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { useScriptsStore } from "../../stores/scriptsStore";
import { languageOptions } from "../../i18n";
import { invokeIpc } from "../../services/ipc/core";
import { getLogLevel, setLogLevel } from "../../services/logging";
import { useToasts } from "../../composables/useToasts";
// Per-view import: this component renders independently from other consumers of this stylesheet.
import "../../styles/settings-tab-switcher.scss";
import UiSelect from "../../components/UiSelect.vue";
import UiSwitch from "../../components/UiSwitch.vue";
import LogViewerDialog from "../../components/LogViewerDialog.vue";
import { createLogger } from "../../utils/logger";

const { t } = useI18n();
const workspace = useWorkspaceStore();
const scriptsStore = useScriptsStore();
const { preferences, proxyConfig } = storeToRefs(workspace);
const { updateIntervalHours } = storeToRefs(scriptsStore);
const logLevel = ref("info");
const logViewerOpen = ref(false);
const proxyPortInput = ref(String(proxyConfig.value.port || 3128));
const savingProxyPort = ref(false);
const proxyPortDirty = ref(false);
const logger = createLogger("frontend.settings.general");
const { showToast } = useToasts();

const logLevelOptions = computed(() => [
  { label: t("settings.general.logLevels.error"), value: "error" },
  { label: t("settings.general.logLevels.warn"), value: "warn" },
  { label: t("settings.general.logLevels.info"), value: "info" },
  { label: t("settings.general.logLevels.debug"), value: "debug" },
  { label: t("settings.general.logLevels.trace"), value: "trace" },
]);

const scriptUpdateIntervalOptions = computed(() => [
  { label: t("settings.general.scriptUpdateIntervals.off"), value: 0 },
  { label: t("settings.general.scriptUpdateIntervals.h12"), value: 12 },
  { label: t("settings.general.scriptUpdateIntervals.h24"), value: 24 },
  { label: t("settings.general.scriptUpdateIntervals.w1"), value: 168 },
]);

if (!scriptsStore.loaded) void scriptsStore.loadScripts();

function updateScriptInterval(hours) {
  void scriptsStore.setUpdateInterval(hours);
}

getLogLevel()
  .then((value) => {
    logLevel.value = value;
  })
  .catch((error) => logger.error("log-level.load.failed", error));

async function updateLogLevel() {
  try {
    logLevel.value = await setLogLevel(logLevel.value);
  } catch (error) {
    logger.error("log-level.update.failed", error);
  }
}

watch(
  () => proxyConfig.value.port,
  (port) => {
    if (!savingProxyPort.value && !proxyPortDirty.value) {
      proxyPortInput.value = String(port);
    }
  },
  { immediate: true },
);

async function commitProxyPort() {
  const nextPort = Number(proxyPortInput.value);
  if (!Number.isInteger(nextPort) || nextPort < 1 || nextPort > 65535) {
    proxyPortInput.value = String(proxyConfig.value.port || 3128);
    proxyPortDirty.value = false;
    showToast({
      type: "error",
      title: t("notifications.proxyPortInvalid"),
    });
    return;
  }
  if (savingProxyPort.value) return;
  if (nextPort === proxyConfig.value.port) {
    proxyPortDirty.value = false;
    return;
  }
  savingProxyPort.value = true;
  try {
    await workspace.updateProxyServerPort(nextPort);
    proxyPortDirty.value = false;
    showToast({
      type: "success",
      title: t("notifications.proxyPortUpdated"),
      message: String(nextPort),
    });
  } catch (error) {
    logger.error("proxy.port.update.failed", error);
    proxyPortInput.value = String(proxyConfig.value.port || 3128);
    proxyPortDirty.value = false;
    showToast({
      type: "error",
      title: t("notifications.proxyPortUpdateFailed"),
      message: String(error),
    });
  } finally {
    savingProxyPort.value = false;
  }
}

function openDeveloperTools() {
  invokeIpc("open_devtools").catch((error) => {
    logger.error("devtools.open.failed", error);
  });
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <Languages
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.general.title") }}
        </h3>
      </div>
    </div>
    <div class="settings-fields">
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.general.language") }}</span>
        <UiSelect
          v-model="preferences.locale"
          :options="languageOptions"
        />
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.general.logLevel") }}</span>
        <UiSelect
          v-model="logLevel"
          :options="logLevelOptions"
          @change="updateLogLevel"
        />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.logs") }}</span>
          <span class="settings-hint">{{ t("settings.general.logsHint") }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="logViewerOpen = true"
        >
          <ScrollText
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.general.viewLogs") }}
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.devTools") }}</span>
          <span class="settings-hint">{{ t("settings.general.devToolsHint") }}</span>
        </div>
        <button
          type="button"
          class="ui-button-secondary flex items-center gap-[6px] text-[0.8571em]"
          @click="openDeveloperTools"
        >
          <Bug
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.general.openDevTools") }}
        </button>
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.showLatency") }}</span>
          <span class="settings-hint">{{ t("settings.general.showLatencyHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.showLatency" />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.proxyToolbarEnabled") }}</span>
          <span class="settings-hint">{{ t("settings.general.proxyToolbarEnabledHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.proxyToolbarEnabled" />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.fileServiceToolbarEnabled") }}</span>
          <span class="settings-hint">{{
            t("settings.general.fileServiceToolbarEnabledHint")
          }}</span>
        </div>
        <UiSwitch v-model="preferences.fileServiceToolbarEnabled" />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.general.proxyPort") }}</span>
          <span class="settings-hint">{{ t("settings.general.proxyPortHint") }}</span>
        </div>
        <input
          v-model="proxyPortInput"
          type="number"
          min="1"
          max="65535"
          class="ui-input ui-input-inline"
          :disabled="savingProxyPort"
          @input="proxyPortDirty = true"
          @change="commitProxyPort"
          @blur="commitProxyPort"
        >
      </div>
      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.general.scriptUpdateInterval") }}</span>
        <ToggleGroupRoot
          :model-value="updateIntervalHours"
          type="single"
          class="settings-tab-switcher"
          @update:model-value="updateScriptInterval"
        >
          <ToggleGroupItem
            v-for="o in scriptUpdateIntervalOptions"
            :key="o.value"
            :value="o.value"
            class="settings-tab-option"
          >
            {{ o.label }}
          </ToggleGroupItem>
        </ToggleGroupRoot>
      </div>
    </div>
    <LogViewerDialog v-model:open="logViewerOpen" />
  </section>
</template>
