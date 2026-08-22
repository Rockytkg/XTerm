<script setup>
import { computed, watch } from "vue";
import { storeToRefs } from "pinia";
import { useWorkspaceStore } from "../stores/workspaceStore";
import SftpWorkspacePane from "./workspace/SftpWorkspacePane.vue";
import TerminalWorkspacePane from "./workspace/TerminalWorkspacePane.vue";
import WorkspaceEmptyState from "./workspace/WorkspaceEmptyState.vue";
import { useWorkspaceDecks } from "./workspace/useWorkspaceDecks";

const workspace = useWorkspaceStore();
const { sessionRuntime } = workspace;

const props = defineProps({
  searchOpenToken: { type: Number, default: 0 },
});

const {
  activeConnectionInfo,
  activeConnectionState,
  activeRemoteWorkingDirectory,
  activeTab,
  activeTerminalSize,
  connections,
  isDark,
  openSessions,
  preferences,
  sessionRecordings,
} = storeToRefs(workspace);

const showEmptyState = computed(
  () =>
    connections.value.length === 0 ||
    openSessions.value.length === 0 ||
    !activeConnectionInfo.value,
);
const {
  activeConnectionId,
  canUseSftp,
  sftpSessions,
  terminalSessions,
  runtimeModeFor,
  terminalOptions,
} = useWorkspaceDecks({
  activeConnectionInfo,
  activeTab,
  isDark,
  openSessions,
  preferences,
  sessionRuntime,
});

watch(canUseSftp, (enabled) => {
  if (!enabled && activeTab.value === "sftp") {
    workspace.selectTab("shell");
  }
});

/**
 * Handles terminal resize events from TerminalWorkspacePane.
 * Guards against invalid dimensions before updating the store.
 * @param {string} sessionId - The terminal session ID that triggered the resize
 * @param {{ cols?: number, rows?: number }} size - The new terminal dimensions
 */
function handleTerminalResize(sessionId, size) {
  if (activeConnectionId.value !== sessionId) return;
  const cols = Math.round(Number(size?.cols));
  const rows = Math.round(Number(size?.rows));
  if (!Number.isFinite(cols) || !Number.isFinite(rows) || cols <= 0 || rows <= 0) return;
  if (activeTerminalSize.value.cols !== cols || activeTerminalSize.value.rows !== rows) {
    activeTerminalSize.value = { cols, rows };
  }
  if (!activeConnectionInfo.value?.sessionId) {
    const status = activeConnectionState.value?.status || "idle";
    if (status === "idle" || status === "failed") {
      workspace.connectTo(activeConnectionInfo.value.connectionId, {
        preserveActiveTab: true,
        sessionId,
      });
    }
  }
}

/**
 * Triggers a forced reconnection to the given connection while preserving the active tab.
 * @param {string} sessionId - The terminal session ID to reconnect
 */
function handleRetryConnection(sessionId) {
  const connectionId =
    openSessions.value.find((session) => session.id === sessionId)?.connectionId || "";
  if (!connectionId) return;
  workspace.connectTo(connectionId, {
    forceReconnect: true,
    preserveActiveTab: true,
    sessionId,
  });
}

/**
 * Checks whether a terminal session currently has an active recording.
 * @param {string} sessionId - The terminal session ID to check
 */
function isRecordingActive(sessionId) {
  return !!sessionRecordings.value.get(sessionId)?.active;
}

/**
 * Forwards a recorded terminal output chunk to the workspace store.
 * @param {string} connectionId - The connection that produced the chunk
 * @param {string} chunk - The recorded terminal output chunk
 */
function handleRecordChunk(connectionId, chunk) {
  workspace.recordTerminalChunk(connectionId, chunk);
}
</script>

<template>
  <div class="flex ui-fill-block flex-col overflow-hidden">
    <WorkspaceEmptyState v-if="showEmptyState" />

    <template v-else>
      <!-- v-show (not v-if): xterm.js addon attaches to the DOM and cannot
           tolerate being removed/re-added on tab switch. Pane stays mounted but hidden. -->
      <TerminalWorkspacePane
        v-show="activeTab === 'shell'"
        :active-connection-id="activeConnectionId"
        :runtime-mode-for="runtimeModeFor"
        :get-recording-active="isRecordingActive"
        :search-open-token="props.searchOpenToken"
        :sessions="terminalSessions"
        :terminal-options="terminalOptions"
        @font-size-change="preferences.terminalFontSize = $event"
        @record-chunk="handleRecordChunk"
        @resize="handleTerminalResize"
        @retry-connection="handleRetryConnection"
        @terminal-ready="workspace.markTerminalPresentationReady"
      />

      <!-- v-if: SFTP pane has no persistent DOM attachment; safe to destroy/recreate on tab switch -->
      <SftpWorkspacePane
        v-if="activeTab === 'sftp' && canUseSftp"
        :active-connection-id="activeConnectionId"
        :sessions="sftpSessions"
        :working-directory="activeRemoteWorkingDirectory"
      />
    </template>
  </div>
</template>
