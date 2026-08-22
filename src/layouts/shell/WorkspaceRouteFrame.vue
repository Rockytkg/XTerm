<script setup>
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { RouterView, useRoute } from "vue-router";
import { storeToRefs } from "pinia";
import { SplitterGroup, SplitterPanel, SplitterResizeHandle } from "reka-ui";
import { useI18n } from "vue-i18n";
import WorkspaceSidebar from "../../components/WorkspaceSidebar.vue";
import WorkspaceTabbar from "../../components/WorkspaceTabbar.vue";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const props = defineProps({
  activePerformanceHistory: { type: Object, default: null },
  handleWorkspaceSplitLayout: { type: Function, default: () => { } },
  handleWorkspaceTabbarAction: { type: Function, default: () => { } },
  onConnectionCreated: { type: Function, default: () => { } },
  onConnectTo: { type: Function, default: () => { } },
  rightSidebarMaxWidth: { type: Number, required: true },
  rightSidebarMinWidth: { type: Number, required: true },
  rightSidebarOpen: { type: Boolean, default: false },
  rightSidebarSearchToken: { type: Number, default: 0 },
  rightSidebarView: { type: String, default: null },
  rightSidebarWidth: { type: Number, required: true },
  tabbarSideButtons: { type: Array, default: () => [] },
});

const { t } = useI18n();
const route = useRoute();
const workspace = useWorkspaceStore();
const {
  activeConnection,
  activeConnectionInfo,
  activeConnectionState,
  activeRemoteWorkingDirectory,
  activeRuntimeMetrics,
  activeTab,
  connectionProfiles,
  openSessions: sessionTabs,
} = storeToRefs(workspace);

const isWorkspace = computed(() => route.name === "workspace");
const isShellWorkspace = computed(() => isWorkspace.value && activeTab.value === "shell");
const showRightSidebar = computed(() => isShellWorkspace.value && props.rightSidebarOpen);
const rightSidebarPanel = ref(null);
const SIDEBAR_SLIDE_MS = 150;
const sidebarExpanded = ref(showRightSidebar.value);
const sidebarCollapsible = ref(!showRightSidebar.value);
const sidebarVisible = ref(showRightSidebar.value);
let sidebarCloseTimer = 0;

function clearSidebarCloseTimer() {
  if (!sidebarCloseTimer) return;
  window.clearTimeout(sidebarCloseTimer);
  sidebarCloseTimer = 0;
}

async function openSidebarPanel() {
  sidebarCollapsible.value = true;
  await nextTick();
  const panel = rightSidebarPanel.value;
  if (!panel) return;
  clearSidebarCloseTimer();
  sidebarExpanded.value = true;
  sidebarVisible.value = false;
  panel.resize(props.rightSidebarWidth);
  panel.expand();
  await nextTick();
  sidebarCollapsible.value = false;
  requestAnimationFrame(() => {
    sidebarVisible.value = true;
  });
}

function closeSidebarPanel() {
  clearSidebarCloseTimer();
  sidebarVisible.value = false;
  sidebarCollapsible.value = true;
  sidebarCloseTimer = window.setTimeout(() => {
    rightSidebarPanel.value?.collapse();
    sidebarExpanded.value = false;
    sidebarCloseTimer = 0;
  }, SIDEBAR_SLIDE_MS);
}

watch(
  showRightSidebar,
  (open) => {
    if (open) {
      void openSidebarPanel();
      return;
    }
    closeSidebarPanel();
  },
  { immediate: true },
);

watch(
  () => props.rightSidebarWidth,
  (width) => {
    if (!sidebarExpanded.value) return;
    rightSidebarPanel.value?.resize(width);
  },
);

onBeforeUnmount(() => {
  clearSidebarCloseTimer();
});
</script>

<template>
  <SplitterGroup
    direction="horizontal"
    class="app-workspace-split"
    @layout="handleWorkspaceSplitLayout"
  >
    <SplitterPanel
      :order="1"
      :min-size="35"
      class="app-main-panel"
    >
      <WorkspaceTabbar
        v-if="isWorkspace"
        :tabs="sessionTabs"
        :active-id="activeConnection"
        :allow-create-connection="activeTab !== 'sftp'"
        :connections="connectionProfiles"
        :connect-protocol-filter="activeTab === 'sftp' ? 'ssh' : ''"
        :side-buttons="tabbarSideButtons"
        :new-connection-label="t('header.newConnection')"
        :no-sessions-label="t('workspace.noActiveSessions')"
        :close-label="t('actions.close')"
        @select="workspace.selectConnection"
        @close="workspace.closeSession"
        @reorder="workspace.reorderSessionTabs"
        @connect="onConnectTo"
        @connection-created="onConnectionCreated"
        @side-action="handleWorkspaceTabbarAction"
      />
      <RouterView v-slot="{ Component }">
        <KeepAlive>
          <component
            :is="Component"
            v-if="route.name === 'workspace'"
            :search-open-token="rightSidebarSearchToken"
          />
        </KeepAlive>
        <component
          :is="Component"
          v-if="route.name !== 'workspace'"
          :search-open-token="rightSidebarSearchToken"
        />
      </RouterView>
    </SplitterPanel>

    <SplitterResizeHandle
      class="app-right-resize-handle"
      :class="{
        'app-right-resize-handle-hidden': !sidebarExpanded,
        'app-right-resize-handle-inert': !sidebarVisible,
      }"
      :disabled="!sidebarVisible"
      :tabindex="sidebarVisible ? 0 : -1"
    >
      <div class="app-right-resize-handle-grip" />
    </SplitterResizeHandle>

    <SplitterPanel
      ref="rightSidebarPanel"
      :collapsible="sidebarCollapsible"
      :collapsed-size="0"
      :order="2"
      :default-size="rightSidebarWidth"
      :min-size="rightSidebarMinWidth"
      :max-size="rightSidebarMaxWidth"
      size-unit="px"
      class="app-right-gutter app-right-gutter-open"
      :class="{
        'app-right-gutter-collapsed': !sidebarExpanded,
        'app-right-gutter-hidden': !sidebarVisible,
      }"
    >
      <aside class="app-right-sidebar-shell">
        <WorkspaceSidebar
          v-show="sidebarExpanded"
          :active-connection="activeConnectionInfo"
          :active-connection-state="activeConnectionState"
          :active-view="rightSidebarView"
          :performance-history="activePerformanceHistory"
          :runtime-metrics="activeRuntimeMetrics"
          :working-directory="activeRemoteWorkingDirectory"
        />
      </aside>
    </SplitterPanel>
  </SplitterGroup>
</template>
