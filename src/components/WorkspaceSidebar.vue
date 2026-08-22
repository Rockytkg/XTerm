<script setup>
import { computed, defineAsyncComponent } from "vue";
import { useI18n } from "vue-i18n";
import { FolderTree } from "@lucide/vue";
const WorkspaceSidebarPerformanceView = defineAsyncComponent(
  () => import("./workspace-sidebar/WorkspaceSidebarPerformanceView.vue"),
);
import WorkspaceSidebarProxyView from "./workspace-sidebar/WorkspaceSidebarProxyView.vue";
import WorkspaceSidebarSessionView from "./workspace-sidebar/WorkspaceSidebarSessionView.vue";
import WorkspaceSidebarFileServiceView from "./workspace-sidebar/WorkspaceSidebarFileServiceView.vue";
import "../styles/workspace-sidebar.scss";

const props = defineProps({
  activeConnection: { type: Object, default: null },
  activeConnectionState: { type: Object, default: () => ({ status: "idle", error: null }) },
  runtimeMetrics: { type: Object, default: null },
  performanceHistory: { type: Object, default: null },
  activeView: { type: String, default: null },
  workingDirectory: { type: String, required: true },
});

const { t } = useI18n();

const hasActiveConnection = computed(() => !!props.activeConnection);
const showServiceView = computed(
  () => props.activeView === "proxy" || props.activeView === "file-service",
);
const latencyMs = computed(() => {
  const latency = Number(props.runtimeMetrics?.latencyMs);
  return Number.isFinite(latency) ? latency : null;
});
</script>

<template>
  <div
    class="workspace-sidebar-panel"
    :class="{
      'workspace-sidebar-panel-performance': activeView === 'performance',
      'workspace-sidebar-panel-proxy': activeView === 'proxy',
      'workspace-sidebar-panel-file-service': activeView === 'file-service',
    }"
  >
    <div
      v-if="!hasActiveConnection && !showServiceView"
      class="workspace-sidebar-empty"
    >
      <FolderTree
        :size="22"
        stroke-width="1.6"
        class="text-text-tertiary"
      />
      <div class="workspace-sidebar-empty-title">
        {{ t("dashboard.emptyTitle") }}
      </div>
      <div class="workspace-sidebar-empty-desc">
        {{ t("dashboard.emptyDesc") }}
      </div>
    </div>

    <template v-else>
      <WorkspaceSidebarSessionView
        v-if="activeView === 'session'"
        :active-connection="activeConnection"
        :active-connection-state="activeConnectionState"
        :working-directory="workingDirectory"
      />

      <WorkspaceSidebarPerformanceView
        v-if="activeView === 'performance'"
        active
        :active-connection="activeConnection"
        :history="performanceHistory"
        :latency-ms="latencyMs"
        :runtime-metrics="runtimeMetrics"
      />

      <WorkspaceSidebarProxyView v-if="activeView === 'proxy'" />

      <WorkspaceSidebarFileServiceView v-if="activeView === 'file-service'" />
    </template>
  </div>
</template>
