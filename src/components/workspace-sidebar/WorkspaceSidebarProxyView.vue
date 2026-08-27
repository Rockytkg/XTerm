<script setup>
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import { Shield } from "@lucide/vue";
import NetworkInterfaceField from "./NetworkInterfaceField.vue";
import UiSwitch from "../UiSwitch.vue";
import { useToasts } from "../../composables/useToasts";
import {
  useNetworkInterfaceOptions,
  useProxyInterfaceRefresh,
} from "../../composables/useNetworkInterfaceOptions";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { createLogger } from "../../utils/logger";
import { formatBytes, formatRate } from "../../utils/formatBytes";

const { t } = useI18n();
const logger = createLogger("frontend.workspace.proxy");
const workspace = useWorkspaceStore();
const { proxyConfig, proxyInterfaces, proxyStats } = storeToRefs(workspace);
const { showToast } = useToasts();

const savingBindIp = ref(false);
const togglingPower = ref(false);

const { interfaceOptions } = useNetworkInterfaceOptions({
  interfaces: proxyInterfaces,
  bindIp: computed(() => proxyConfig.value.bindIp),
  staleLabel: computed(() => t("sidebar.proxy.staleInterface")),
});
const { refreshingInterfaces, refreshInterfaces } = useProxyInterfaceRefresh({ workspace });

const powerValue = computed({
  get: () => !!proxyConfig.value.running,
  set: (value) => {
    void togglePower(Boolean(value));
  },
});

const statusTone = computed(() => {
  if (proxyConfig.value.running) return "workspace-sidebar-status-online";
  return "";
});

const totalTraffic = computed(
  () =>
    (Number(proxyStats.value?.uploadBytesTotal) || 0) +
    (Number(proxyStats.value?.downloadBytesTotal) || 0),
);
const downloadRateLabel = computed(() =>
  formatRate(proxyStats.value?.downloadBytesPerSec || 0),
);
const uploadRateLabel = computed(() => formatRate(proxyStats.value?.uploadBytesPerSec || 0));
const totalTrafficLabel = computed(() => formatBytes(totalTraffic.value));

async function togglePower(enabled) {
  if (togglingPower.value) return;
  togglingPower.value = true;
  try {
    if (enabled) {
      await workspace.startProxyServer(proxyConfig.value.port, proxyConfig.value.bindIp);
      showToast({
        type: "success",
        title: t("notifications.proxyStarted"),
        message: `${proxyConfig.value.bindIp}:${proxyConfig.value.port}`,
      });
    } else {
      await workspace.stopProxyServer();
      showToast({
        type: "success",
        title: t("notifications.proxyStopped"),
      });
    }
  } catch (error) {
    logger.error("proxy.toggle.failed", error);
    showToast({
      type: "error",
      title: enabled ? t("notifications.proxyStartFailed") : t("notifications.proxyStopFailed"),
      message: String(error),
    });
  } finally {
    togglingPower.value = false;
  }
}

async function updateBindIp(bindIp) {
  if (!bindIp || bindIp === proxyConfig.value.bindIp || savingBindIp.value) return;
  savingBindIp.value = true;
  try {
    await workspace.updateProxyServerBindIp(bindIp);
    showToast({
      type: "success",
      title: t("notifications.proxyBindIpUpdated"),
      message: bindIp,
    });
  } catch (error) {
    logger.error("proxy.bind_ip.update.failed", error);
    showToast({
      type: "error",
      title: t("notifications.proxyBindIpUpdateFailed"),
      message: String(error),
    });
  } finally {
    savingBindIp.value = false;
  }
}
</script>

<template>
  <div class="workspace-sidebar-pane">
    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-section-head">
        <div class="workspace-sidebar-section-icon">
          <Shield
            :size="16"
            stroke-width="1.8"
          />
        </div>
        <div class="min-w-0">
          <div class="workspace-sidebar-section-kicker">
            {{ t("sidebar.proxy.kicker") }}
          </div>
          <div class="workspace-sidebar-section-title">
            {{ t("sidebar.proxy.title") }}
          </div>
        </div>
        <span
          class="workspace-sidebar-status-pill"
          :class="statusTone"
        >
          {{ proxyConfig.running ? t("sidebar.proxy.running") : t("sidebar.proxy.stopped") }}
        </span>
      </div>

      <div class="workspace-sidebar-endpoint">
        {{ proxyConfig.bindIp }}:{{ proxyConfig.port }}
      </div>

      <div class="workspace-sidebar-metric-grid">
        <div class="workspace-sidebar-metric">
          <span>{{ t("sidebar.proxy.stats.downloadRate") }}</span>
          <strong>{{ downloadRateLabel }}</strong>
        </div>
        <div class="workspace-sidebar-metric">
          <span>{{ t("sidebar.proxy.stats.uploadRate") }}</span>
          <strong>{{ uploadRateLabel }}</strong>
        </div>
        <div class="workspace-sidebar-metric">
          <span>{{ t("sidebar.proxy.stats.bytes") }}</span>
          <strong>{{ totalTrafficLabel }}</strong>
        </div>
      </div>
    </section>

    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-pref-list">
        <div class="workspace-sidebar-pref-row">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{ t("sidebar.proxy.power") }}</span>
            <span class="workspace-sidebar-pref-hint">{{ t("sidebar.proxy.powerHint") }}</span>
          </div>
          <UiSwitch
            v-model="powerValue"
            :disabled="togglingPower"
          />
        </div>

        <NetworkInterfaceField
          :bind-ip="proxyConfig.bindIp"
          :disabled="savingBindIp || togglingPower"
          :hint="t('sidebar.proxy.bindIpHint')"
          :label="t('sidebar.proxy.bindIp')"
          :options="interfaceOptions"
          :refresh-disabled="refreshingInterfaces"
          :refresh-label="t('sidebar.proxy.refreshInterfaces')"
          :refreshing="refreshingInterfaces"
          @refresh="refreshInterfaces"
          @update:bind-ip="updateBindIp"
        />
      </div>
    </section>
  </div>
</template>
