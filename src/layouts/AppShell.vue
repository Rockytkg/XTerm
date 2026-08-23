<script setup>
import { onBeforeUnmount, onMounted } from "vue";
import { useRouter } from "vue-router";
import { storeToRefs } from "pinia";
import { getCurrentWindow } from "@tauri-apps/api/window";
import HostKeyDialog from "../components/HostKeyDialog.vue";
import ScriptPromptDialog from "../components/ScriptPromptDialog.vue";
import ScriptRunPickerDialog from "../components/ScriptRunPickerDialog.vue";
import SshCredentialPromptDialog from "../components/SshCredentialPromptDialog.vue";
import { useI18n } from "vue-i18n";
import { useWorkspacePerformanceHistory } from "../composables/useWorkspacePerformanceHistory";
import { useWorkspaceShellController } from "../composables/useWorkspaceShellController";
import { dismissContextMenu } from "../services/contextMenu";
import { invokeIpc } from "../services/ipc/core";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { blurActiveElement } from "../utils/focusGuards";
import { connectionCan } from "../utils/connectionCapabilities";
import { createShortcutRegistry } from "../utils/shortcutRegistry";
import { createLogger } from "../utils/logger";
import ShellPrimaryNav from "./shell/ShellPrimaryNav.vue";
import ShellStatusBar from "./shell/ShellStatusBar.vue";
import ShellTitlebar from "./shell/ShellTitlebar.vue";
import WorkspaceRouteFrame from "./shell/WorkspaceRouteFrame.vue";
import "../styles/context-menu.scss";
import "../styles/shell-layout.scss";

const { t } = useI18n();
const logger = createLogger("frontend.shell.app");
const router = useRouter();
const workspace = useWorkspaceStore();
const {
  activeConnection,
  activeConnectionInfo,
  activeRuntimeMetrics,
  activeTab,
  hostKeyPrompt,
  lastSerialBaudEvent,
  openSessions: sessionTabs,
  preferences,
  sessionRecordings,
  sshCredentialPrompt,
} = storeToRefs(workspace);

function navigate(name) {
  router.push({ name });
}

const {
  handleWorkspaceSplitLayout,
  handleWorkspaceTabbarAction,
  navItems,
  onConnectionCreated,
  onConnectTo,
  rightSidebarMaxWidth,
  rightSidebarMinWidth,
  rightSidebarOpen,
  rightSidebarView,
  rightSidebarWidth,
  rightSidebarSearchToken,
  toggleActiveSessionRecording,
  tabbarSideButtons,
} = useWorkspaceShellController({
  activeConnection,
  activeConnectionInfo,
  activeTab,
  lastSerialBaudEvent,
  preferences,
  sessionRecordings,
  sessionTabs,
  connectTo: workspace.connectTo,
  reconnectSerialAutoBaud: workspace.reconnectSerialAutoBaud,
  refreshConnectionList: workspace.refreshConnectionList,
  selectTab: workspace.selectTab,
  toggleSessionRecording: workspace.toggleSessionRecording,
  navigate,
});

const { activePerformanceHistory } = useWorkspacePerformanceHistory({
  activeConnectionInfo,
  runtimeMetrics: activeRuntimeMetrics,
  openSessions: sessionTabs,
});

const globalShortcuts = createShortcutRegistry();
globalShortcuts.register({
  id: "open-devtools",
  shortcut: () => preferences.value.openDevToolsShortcut,
  run: () => {
    invokeIpc("open_devtools").catch((error) => {
      logger.error("devtools.open.failed", error);
    });
  },
});
globalShortcuts.register({
  id: "serial-redetect-baud",
  shortcut: () => preferences.value.serialRedetectBaudShortcut,
  when: () =>
    connectionCan(activeConnectionInfo.value, "serialBaudDetection") &&
    activeConnectionInfo.value?.baudRate === "auto",
  run: () => handleWorkspaceTabbarAction("quick-redetect-baud"),
});
globalShortcuts.register({
  id: "session-recording",
  shortcut: () => preferences.value.sessionRecordingShortcut,
  when: () => !!activeConnectionInfo.value && activeTab.value === "shell",
  run: () => toggleActiveSessionRecording(),
});

const asyncListeners = createAsyncListenerRegistry();

function dismissTransientFocus() {
  // 失焦时菜单不能继续覆盖其他窗口；先关闭菜单，再清理普通焦点。
  dismissContextMenu();
  blurActiveElement({
    exclude: (element) => element.closest?.(".dialog-content"),
  });
}

onMounted(() => {
  globalShortcuts.attach(document);
  asyncListeners.register(
    getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) dismissTransientFocus();
    }),
  );
});

onBeforeUnmount(() => {
  globalShortcuts.dispose();
  asyncListeners.dispose();
});
</script>

<template>
  <div class="shell-root">
    <ShellTitlebar />

    <div class="shell-content-row">
      <ShellPrimaryNav :nav-items="navItems" />
      <WorkspaceRouteFrame
        :active-performance-history="activePerformanceHistory"
        :handle-workspace-split-layout="handleWorkspaceSplitLayout"
        :handle-workspace-tabbar-action="handleWorkspaceTabbarAction"
        :on-connection-created="onConnectionCreated"
        :on-connect-to="onConnectTo"
        :right-sidebar-max-width="rightSidebarMaxWidth"
        :right-sidebar-min-width="rightSidebarMinWidth"
        :right-sidebar-open="rightSidebarOpen"
        :right-sidebar-search-token="rightSidebarSearchToken"
        :right-sidebar-view="rightSidebarView"
        :right-sidebar-width="rightSidebarWidth"
        :tabbar-side-buttons="tabbarSideButtons"
      />
      <div
        class="shell-right-edge"
        aria-hidden="true"
      />
    </div>

    <ShellStatusBar />

    <HostKeyDialog
      v-if="hostKeyPrompt"
      :prompt="hostKeyPrompt"
      :title="t('hostKeyPrompt.title')"
      :description="
        t('hostKeyPrompt.description', { host: hostKeyPrompt.host, port: hostKeyPrompt.port })
      "
      :algorithm-label="t('hostKeyPrompt.algorithm')"
      :fingerprint-label="t('hostKeyPrompt.fingerprint')"
      :cancel-label="t('hostKeyPrompt.cancel')"
      :once-label="t('hostKeyPrompt.once')"
      :save-label="t('hostKeyPrompt.save')"
      @answer="workspace.answerHostKeyPrompt"
    />

    <SshCredentialPromptDialog
      v-if="sshCredentialPrompt"
      :prompt="sshCredentialPrompt"
      @cancel="workspace.cancelSshCredentialPrompt"
      @submit="workspace.answerSshCredentialPrompt"
    />

    <ScriptPromptDialog />
    <ScriptRunPickerDialog />
  </div>
</template>
