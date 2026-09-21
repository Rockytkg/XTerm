<script setup>
import { computed, defineAsyncComponent } from "vue";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { capabilitiesCan } from "../../utils/connectionCapabilities";
import { isRdpProtocol, isVncProtocol } from "../../utils/connectionProtocols";

const TerminalPanel = defineAsyncComponent(() => import("../../components/TerminalPanel.vue"));
const VncDesktopPane = defineAsyncComponent(() => import("../../components/VncDesktopPane.vue"));
const RdpDesktopPane = defineAsyncComponent(() => import("../../components/RdpDesktopPane.vue"));

const props = defineProps({
  activeConnectionId: { type: String, default: "" },
  runtimeModeFor: { type: Function, required: true },
  getRecordingActive: { type: Function, default: () => false },
  searchOpenToken: { type: Number, default: 0 },
  sessions: { type: Array, default: () => [] },
  terminalOptions: { type: Object, default: () => ({}) },
});

const emit = defineEmits([
  "fontSizeChange",
  "recordChunk",
  "resize",
  "retryConnection",
  "terminalReady",
]);

const { sessionRuntime } = useWorkspaceStore();

const sessionsWithRuntime = computed(() =>
  props.sessions.map((session) => ({
    ...session,
    ...sessionRuntime(session.id),
  })),
);

// 桌面会话优先按 open 响应回读的 capabilities.video 判定；连接中 capabilities
// 尚未到达时回退到协议判断，保证 VNC/RDP 在握手期间也能渲染桌面面板。
const isDesktopSession = (session) =>
  capabilitiesCan(session.capabilities, "video") ||
  isVncProtocol(session.protocol) ||
  isRdpProtocol(session.protocol);

const desktopSessions = computed(() => sessionsWithRuntime.value.filter(isDesktopSession));
const vncDesktopSessions = computed(() =>
  desktopSessions.value.filter((session) => isVncProtocol(session.protocol)),
);
const rdpDesktopSessions = computed(() =>
  desktopSessions.value.filter((session) => isRdpProtocol(session.protocol)),
);
const terminalSessions = computed(() =>
  sessionsWithRuntime.value.filter((session) => !isDesktopSession(session)),
);
</script>

<template>
  <section class="relative flex flex-1 min-h-0 overflow-hidden">
    <!-- 桌面会话渲染远程桌面面板替代终端；tab 切换复用会话切换。 -->
    <VncDesktopPane
      v-for="session in vncDesktopSessions"
      v-show="session.id === activeConnectionId"
      :key="session.terminalKey || session.id"
      class="absolute inset-0"
      :active-connection="session"
      :connection-state="session.connectionState"
      :vnc-bridge="session.vncBridge"
      @retry-connection="(options) => emit('retryConnection', session.id, options)"
    />
    <RdpDesktopPane
      v-for="session in rdpDesktopSessions"
      v-show="session.id === activeConnectionId"
      :key="session.terminalKey || session.id"
      class="absolute inset-0"
      :active-connection="session"
      :connection-state="session.connectionState"
      :frontend-session-id="session.id"
      :rdp-bridge="session.rdpBridge"
      @retry-connection="(options) => emit('retryConnection', session.id, options)"
    />
    <TerminalPanel
      v-for="session in terminalSessions"
      v-show="session.id === activeConnectionId"
      :key="session.terminalKey || session.id"
      v-bind="terminalOptions"
      class="absolute inset-0"
      :active-connection="session"
      :connection-state="session.connectionState"
      :session-id="session.sessionId || ''"
      :visible="session.id === activeConnectionId"
      :runtime-mode="runtimeModeFor(session.id)"
      :recording-active="getRecordingActive(session.id)"
      :search-open-token="searchOpenToken"
      @terminal-font-size-change="emit('fontSizeChange', $event)"
      @terminal-resize="emit('resize', session.id, $event)"
      @terminal-record-chunk="emit('recordChunk', session.id, $event)"
      @terminal-ready="emit('terminalReady', session.id)"
      @retry-connection="emit('retryConnection', session.id)"
    />
  </section>
</template>
