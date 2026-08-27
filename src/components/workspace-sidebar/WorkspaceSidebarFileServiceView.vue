<script setup>
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import { Files, FolderOpen, Server, Trash2 } from "@lucide/vue";
import NetworkInterfaceField from "./NetworkInterfaceField.vue";
import UiSwitch from "../UiSwitch.vue";
import { useToasts } from "../../composables/useToasts";
import {
  useNetworkInterfaceOptions,
  useProxyInterfaceRefresh,
} from "../../composables/useNetworkInterfaceOptions";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { chooseSharedDirectory } from "../../services/fileService";
import { formatBytes } from "../../utils/formatBytes";
import { createLogger } from "../../utils/logger";

const { t } = useI18n();
const logger = createLogger("frontend.workspace.file_service_view");
const workspace = useWorkspaceStore();
const { proxyInterfaces, fileServiceConfig, fileServiceTransfers } = storeToRefs(workspace);
const { showToast } = useToasts();
const busy = ref(false);
const savingBindIp = ref(false);
const savingDirectory = ref(false);
const selectedProtocol = ref("tftp");

const protocolOptions = [
  { value: "tftp", label: "TFTP" },
  { value: "ftp", label: "FTP" },
  { value: "sftp", label: "SFTP" },
];

watch(
  () => fileServiceConfig.value.protocol,
  (protocol) => {
    selectedProtocol.value = protocol || "tftp";
  },
  { immediate: true },
);

const { interfaceOptions } = useNetworkInterfaceOptions({
  interfaces: proxyInterfaces,
  bindIp: computed(() => fileServiceConfig.value.bindIp),
  staleLabel: computed(() => t("sidebar.fileService.staleInterface")),
});
const { refreshingInterfaces, refreshInterfaces } = useProxyInterfaceRefresh({ workspace });
const historyTransfers = computed(() =>
  fileServiceTransfers.value.filter((item) => item.done || item.error),
);
const statusTone = computed(() =>
  fileServiceConfig.value.running ? "workspace-sidebar-status-online" : "",
);
const isRunning = computed({
  get: () => !!fileServiceConfig.value.running,
  set: (value) => void toggleService(value),
});

function selectProtocol(protocol) {
  selectedProtocol.value = protocol;
}

function transferPercent(transfer) {
  if (transfer.done) return 100;
  const total = Number(transfer.total) || 0;
  return total > 0 ? Math.min(99, Math.round((Number(transfer.transferred) / total) * 100)) : 0;
}

function transferDirection(transfer) {
  return transfer.direction === "write" ? "upload" : "download";
}

function transferDirectionLabel(transfer) {
  return t(`sidebar.fileService.direction.${transfer.direction === "write" ? "write" : "read"}`);
}

async function toggleService(enabled) {
  if (busy.value) return;
  busy.value = true;
  try {
    if (!enabled) {
      await workspace.stopFileServiceServer();
    } else {
      await workspace.startFileServiceServer(selectedProtocol.value);
    }
    showToast({
      type: "success",
      title: t(enabled ? "notifications.fileServiceStarted" : "notifications.fileServiceStopped"),
    });
  } catch (error) {
    logger.error("file_service.toggle.failed", error);
    showToast({
      type: "error",
      title: t("notifications.fileServiceOperationFailed"),
      message: String(error),
    });
  } finally {
    busy.value = false;
  }
}

async function updateBindIp(bindIp) {
  if (!bindIp || bindIp === fileServiceConfig.value.bindIp || savingBindIp.value) return;
  savingBindIp.value = true;
  try {
    await workspace.updateFileServiceBindIp(bindIp);
  } catch (error) {
    logger.error("file_service.bind_ip.update.failed", error);
    showToast({
      type: "error",
      title: t("notifications.fileServiceOperationFailed"),
      message: String(error),
    });
  } finally {
    savingBindIp.value = false;
  }
}

async function browseDirectory() {
  if (savingDirectory.value) return;
  savingDirectory.value = true;
  try {
    const selected = await chooseSharedDirectory(
      fileServiceConfig.value.sharedDir,
      t("sidebar.fileService.chooseDirectory"),
    );
    if (selected) await workspace.updateFileServiceSharedDir(selected);
  } finally {
    savingDirectory.value = false;
  }
}
</script>

<template>
  <div class="workspace-sidebar-pane workspace-sidebar-pane-file-service">
    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-section-head">
        <div class="workspace-sidebar-section-icon">
          <Server
            :size="16"
            stroke-width="1.8"
          />
        </div>
        <div class="min-w-0">
          <div class="workspace-sidebar-section-kicker">
            {{ t("sidebar.fileService.kicker") }}
          </div>
          <div class="workspace-sidebar-section-title">
            {{ t("sidebar.fileService.title") }}
          </div>
        </div>
        <span
          class="workspace-sidebar-status-pill"
          :class="statusTone"
        >
          {{
            fileServiceConfig.running
              ? t("sidebar.fileService.running")
              : t("sidebar.fileService.stopped")
          }}
        </span>
      </div>
      <div
        class="workspace-sidebar-endpoint"
        :title="`${fileServiceConfig.bindIp}:${fileServiceConfig.port}`"
      >
        {{ fileServiceConfig.bindIp }}:{{ fileServiceConfig.port }}
      </div>
    </section>

    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-pref-list">
        <div class="workspace-sidebar-pref-row">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{ t("sidebar.fileService.power") }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("sidebar.fileService.powerHint")
            }}</span>
          </div>
          <UiSwitch
            v-model="isRunning"
            :disabled="busy"
          />
        </div>

        <div class="workspace-sidebar-pref-row workspace-sidebar-pref-row-stack">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("sidebar.fileService.protocol")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("sidebar.fileService.protocolHint")
            }}</span>
          </div>
          <div
            class="file-service-protocols"
            role="radiogroup"
            :aria-label="t('sidebar.fileService.protocol')"
          >
            <button
              v-for="option in protocolOptions"
              :key="option.value"
              type="button"
              role="radio"
              class="file-service-protocol"
              :class="{ 'is-selected': selectedProtocol === option.value }"
              :aria-checked="selectedProtocol === option.value"
              :disabled="busy || fileServiceConfig.running"
              @click="selectProtocol(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <NetworkInterfaceField
          :bind-ip="fileServiceConfig.bindIp"
          :disabled="busy || savingBindIp"
          :hint="t('sidebar.fileService.bindIpHint')"
          :label="t('sidebar.fileService.bindIp')"
          :options="interfaceOptions"
          :refresh-disabled="refreshingInterfaces"
          :refresh-label="t('sidebar.fileService.refreshInterfaces')"
          :refreshing="refreshingInterfaces"
          @refresh="refreshInterfaces"
          @update:bind-ip="updateBindIp"
        />

        <div class="workspace-sidebar-pref-row">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("sidebar.fileService.sharedDirectory")
            }}</span>
            <span
              class="workspace-sidebar-pref-hint file-service-directory-path"
              :title="fileServiceConfig.sharedDir || ''"
            >
              {{ fileServiceConfig.sharedDir || t("sidebar.fileService.noDirectory") }}
            </span>
          </div>
          <button
            type="button"
            class="ui-icon-button shrink-0"
            :aria-label="t('sidebar.fileService.chooseDirectory')"
            :title="t('sidebar.fileService.chooseDirectory')"
            :disabled="savingDirectory || busy"
            @click="browseDirectory"
          >
            <FolderOpen :size="16" />
          </button>
        </div>
      </div>
    </section>

    <section class="workspace-sidebar-section file-service-transfers">
      <div class="file-service-transfers-head">
        <div class="workspace-sidebar-section-kicker">
          {{ t("sidebar.fileService.transfers") }}
        </div>
        <span
          v-if="fileServiceTransfers.length"
          class="file-service-transfers-count"
        >
          {{ fileServiceTransfers.length }}
        </span>
        <button
          v-if="historyTransfers.length"
          type="button"
          class="ui-icon-button"
          :aria-label="t('sidebar.fileService.clearHistory')"
          :title="t('sidebar.fileService.clearHistory')"
          @click="workspace.clearFileTransfers"
        >
          <Trash2 :size="14" />
        </button>
      </div>

      <div
        v-if="!fileServiceTransfers.length"
        class="file-service-empty-state"
      >
        <Files
          :size="20"
          stroke-width="1.6"
        />
        <span>{{ t("sidebar.fileService.noTransfersHint") }}</span>
      </div>
      <div
        v-else
        class="file-service-transfer-list"
      >
        <div
          v-for="transfer in fileServiceTransfers"
          :key="transfer.id"
          class="file-service-transfer-row"
          :class="{
            'is-upload': transferDirection(transfer) === 'upload',
            'is-download': transferDirection(transfer) === 'download',
            'is-error': transfer.error,
          }"
        >
          <div class="file-service-transfer-topline">
            <strong :title="transfer.name">{{ transfer.name }}</strong>
            <span class="file-service-transfer-percent">{{ transferPercent(transfer) }}%</span>
          </div>
          <div class="file-service-transfer-track">
            <span :style="{ width: `${transferPercent(transfer)}%` }" />
          </div>
          <div class="file-service-transfer-meta">
            <span class="file-service-transfer-direction">{{
              transferDirectionLabel(transfer)
            }}</span>
            <span class="file-service-transfer-peer">{{ transfer.peer || "-" }}</span>
            <span class="file-service-transfer-size">{{
              formatBytes(transfer.transferred)
            }}</span>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>
