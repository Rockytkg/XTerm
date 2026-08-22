<script setup>
import { computed } from "vue";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { connectionEndpointLabel } from "../../utils/connectionProtocols";

const { t } = useI18n();
const workspace = useWorkspaceStore();
const {
  activeConnectionInfo,
  activeRuntimeMetrics,
  activeTerminalSize,
  activeRemoteWorkingDirectory,
  openSessions,
  preferences,
} = storeToRefs(workspace);

const activeEndpoint = computed(() => {
  if (!activeConnectionInfo.value) return "";
  return connectionEndpointLabel(activeConnectionInfo.value);
});

const activeLatencyText = computed(() => {
  const latency = Number(activeRuntimeMetrics.value?.latencyMs);
  return Number.isFinite(latency) ? `${latency.toFixed(1)} ms` : "-";
});

const activeDirectoryText = computed(() => {
  const directory = String(activeRemoteWorkingDirectory.value || "").trim();
  return directory || "-";
});

const activeLatencyClass = computed(() => {
  const latency = Number(activeRuntimeMetrics.value?.latencyMs);
  if (!Number.isFinite(latency)) return "shell-status-latency-unknown";
  if (latency < 80) return "shell-status-latency-good";
  if (latency < 180) return "shell-status-latency-fair";
  return "shell-status-latency-poor";
});

function statusClass(status) {
  if (status === "online") return "ui-status-dot-online";
  if (status === "warning") return "ui-status-dot-warning";
  return "ui-status-dot-offline";
}
</script>

<template>
  <footer
    class="row-start-3 flex h-[var(--shell-statusbar-height)] select-none items-center justify-between border-t border-border bg-bg-secondary px-[12px] text-[0.7857em] text-text-tertiary"
  >
    <div class="flex items-center gap-[6px]">
      <template v-if="activeConnectionInfo">
        <span
          class="ui-status-dot"
          :class="statusClass(activeConnectionInfo.status)"
        />
        <span class="text-[0.7857em] text-text-tertiary">{{ activeEndpoint }}</span>
        <span class="h-[10px] w-[1px] bg-border-light" />
      </template>
      <span
        v-else
        class="text-[0.7857em] text-text-tertiary"
      >
        {{ t("workspace.noActiveSessions") }}
      </span>
      <span class="text-[0.7857em] text-text-tertiary">{{ activeDirectoryText }}</span>
      <span
        v-if="preferences.showLatency && activeConnectionInfo"
        class="h-[10px] w-[1px] bg-border-light"
      />
      <span
        v-if="preferences.showLatency && activeConnectionInfo"
        class="text-[0.7857em]"
        :class="activeLatencyClass"
      >
        {{ activeLatencyText }}
      </span>
    </div>
    <div class="flex items-center gap-[6px]">
      <template v-if="activeConnectionInfo && activeTerminalSize.cols > 0">
        <span class="text-[0.7857em] text-text-tertiary">
          {{ activeTerminalSize.cols }}×{{ activeTerminalSize.rows }}
        </span>
        <span class="h-[10px] w-[1px] bg-border-light" />
        <span class="text-[0.7857em] text-text-tertiary">{{ preferences.terminalFontSize }}px</span>
        <span class="h-[10px] w-[1px] bg-border-light" />
      </template>
      <span class="text-[0.7857em] text-text-tertiary">
        {{ t("workspace.activeConnections", { count: openSessions.length }) }}
      </span>
    </div>
  </footer>
</template>

<style scoped>
.shell-status-latency-unknown {
  color: var(--text-tertiary);
}

.shell-status-latency-good {
  color: var(--success);
}

.shell-status-latency-fair {
  color: var(--warning);
}

.shell-status-latency-poor {
  color: var(--danger);
}
</style>
