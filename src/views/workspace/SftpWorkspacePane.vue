<script setup>
import { computed } from "vue";
import SftpPanel from "../../components/SftpPanel.vue";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const props = defineProps({
  activeConnectionId: { type: String, default: "" },
  sessions: { type: Array, default: () => [] },
  workingDirectory: { type: String, default: "" },
});

const { sessionRuntime } = useWorkspaceStore();

const activeSession = computed(
  () => props.sessions.find((session) => session.id === props.activeConnectionId) || null,
);
const activeRuntime = computed(() =>
  activeSession.value ? sessionRuntime(activeSession.value.id) : null,
);
const activeConnection = computed(() =>
  activeSession.value && activeRuntime.value
    ? {
        ...activeSession.value,
        ...activeRuntime.value,
        id: activeSession.value.connectionId || activeSession.value.id,
        sessionId: activeSession.value.sessionId || "",
      }
    : null,
);
</script>

<template>
  <section class="relative flex flex-1 min-h-0 overflow-hidden">
    <SftpPanel
      v-if="activeSession"
      :key="activeSession.id"
      class="absolute inset-0"
      :connection="activeConnection"
      :session-id="activeSession.sessionId || ''"
      :working-directory="workingDirectory"
      visible
    />
  </section>
</template>
