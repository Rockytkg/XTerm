<script setup>
import { computed, ref } from "vue";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { Trash2, LoaderCircle } from "@lucide/vue";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { connectionEndpointLabel } from "../../utils/connectionProtocols";
import { useQuickButtons } from "../../composables/useQuickButtons";
import { expandTerminalEscapes, splitTerminalSendContent } from "../../utils/terminalEscapes";
import QuickButtonDialog from "../../components/QuickButtonDialog.vue";
import ConfirmDialog from "../../components/ConfirmDialog.vue";
import { getScriptBridge } from "../../services/scripting/bridges";
import { useScriptsStore } from "../../stores/scriptsStore";
import { useScriptExecution } from "../../composables/useScriptExecution";
import { openContextMenu } from "../../services/contextMenu";
import { SCRIPT_RUN_STATUS, scriptRuns } from "../../services/scripting/scriptRunner";

const { t } = useI18n();
const workspace = useWorkspaceStore();
const scriptsStore = useScriptsStore();
if (!scriptsStore.loaded) void scriptsStore.loadScripts();
const { runScriptOnActiveSession } = useScriptExecution();
const { buttons, loadQuickButtons, remove } = useQuickButtons();
void loadQuickButtons();
const dialogOpen = ref(false);
const editingButton = ref(null);
const deleteTarget = ref(null);
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

// 正在运行的脚本：状态栏右侧给出常驻指示，脚本结束后自动消失。
const runningScripts = computed(() =>
  scriptRuns.value.filter((run) => run.status === SCRIPT_RUN_STATUS.RUNNING),
);

const runningScriptNames = computed(() =>
  runningScripts.value.map((run) => run.scriptName || t("scripts.untitled")).join(", "),
);

const runningScriptsText = computed(() => {
  if (runningScripts.value.length === 1) {
    return t("statusBar.scriptRunning", {
      name: runningScripts.value[0].scriptName || t("scripts.untitled"),
    });
  }
  return t("statusBar.scriptsRunning", { count: runningScripts.value.length });
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

function buttonMenu(event, button = null) {
  const items = button
    ? [
        {
          id: "quick-edit",
          label: t("statusBar.quickButtons.edit"),
          icon: "edit",
          enabled: true,
          action: () => {
            editingButton.value = { ...button };
            dialogOpen.value = true;
          },
        },
        {
          id: "quick-delete",
          label: t("statusBar.quickButtons.delete"),
          icon: "delete",
          enabled: true,
          action: () => {
            deleteTarget.value = button;
          },
        },
      ]
    : [
        {
          id: "quick-new",
          label: t("statusBar.quickButtons.add"),
          icon: "newFile",
          enabled: true,
          action: () => {
            editingButton.value = null;
            dialogOpen.value = true;
          },
        },
      ];
  openContextMenu(event, { items, suppressDefaultEditItems: true });
}

function confirmDelete() {
  if (deleteTarget.value) remove(deleteTarget.value.id);
  deleteTarget.value = null;
}

// 发送内容按 \d毫秒 切分后逐段下发，delay 段通过 setTimeout 暂停，
// 文本段转义展开后经桥发送（等同键入，走正常输入链路）。
async function sendButtonContent(sessionId, value) {
  const bridge = getScriptBridge(sessionId);
  if (!bridge) return;
  for (const segment of splitTerminalSendContent(value)) {
    if (segment.type === "delay") {
      await new Promise((resolve) => setTimeout(resolve, segment.ms));
    } else {
      bridge.send(expandTerminalEscapes(segment.text));
    }
  }
}

function runButton(button) {
  const sessionId = activeConnectionInfo.value?.id || workspace.activeConnection;
  if (!sessionId) return;
  if (button.type === "script") {
    const script = scriptsStore.scripts.find((item) => item.id === button.value);
    if (script) void runScriptOnActiveSession(script);
  } else void sendButtonContent(sessionId, button.value);
  // 点击后焦点还在按钮上，交还给终端（脚本可能弹交互对话框，对话框会自行抢焦点）。
  getScriptBridge(sessionId)?.focus?.();
}
</script>

<template>
  <footer
    class="row-start-3 flex h-[var(--shell-statusbar-height)] select-none items-center justify-between border-t border-border bg-bg-secondary px-[12px] text-[0.7857em] text-text-tertiary"
    @contextmenu="buttonMenu"
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
      <button
        v-for="button in buttons"
        :key="button.id"
        type="button"
        class="shell-status-quick-button"
        :title="button.name"
        @mousedown.prevent
        @click="runButton(button)"
        @contextmenu.prevent.stop="buttonMenu($event, button)"
      >
        <span
          class="shell-status-quick-button-dot"
          :style="{ backgroundColor: button.color }"
        />
        <span class="shell-status-quick-button-label">{{ button.name }}</span>
      </button>
    </div>
    <div class="flex items-center gap-[6px]">
      <template v-if="runningScripts.length">
        <LoaderCircle
          :size="12"
          stroke-width="2"
          class="animate-spin text-text-secondary"
        />
        <span
          class="max-w-[180px] overflow-hidden text-ellipsis whitespace-nowrap text-[0.7857em] text-text-secondary"
          :title="runningScriptNames"
        >
          {{ runningScriptsText }}
        </span>
        <span class="h-[10px] w-[1px] bg-border-light" />
      </template>
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
  <QuickButtonDialog
    v-model:open="dialogOpen"
    :button="editingButton"
  />
  <ConfirmDialog
    :open="Boolean(deleteTarget)"
    tone="danger"
    :title="t('statusBar.quickButtons.confirmDeleteTitle')"
    :description="
      t('statusBar.quickButtons.confirmDeleteDescription', { name: deleteTarget?.name || '' })
    "
    :confirm-text="t('statusBar.quickButtons.confirmDeleteConfirm')"
    :confirm-icon="Trash2"
    @update:open="
      (v) => {
        if (!v) deleteTarget = null;
      }
    "
    @confirm="confirmDelete"
  />
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

.shell-status-quick-button {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  min-width: 0;
  max-width: 180px;
  height: 20px;
  padding: 0 var(--space-2);
  border: 0;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  background: transparent;
  font: inherit;
  cursor: pointer;
  transition:
    background-color var(--motion-duration-quick) var(--ease-default),
    color var(--motion-duration-quick) var(--ease-default);
}

.shell-status-quick-button:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.shell-status-quick-button-dot {
  width: 8px;
  height: 8px;
  flex: 0 0 8px;
  border-radius: var(--radius-pill);
}

.shell-status-quick-button-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
