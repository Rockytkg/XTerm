import { computed } from "vue";

export function useTerminalRuntimeDeck({ openSessions, activeConnectionId, workspaceActive }) {
  const openSessionIds = computed(() =>
    openSessions.value.map((session) => session.id).filter(Boolean),
  );

  const mountedSessionIds = computed(() => openSessionIds.value);

  function runtimeModeFor(connectionId) {
    if (!connectionId) return "inactive";
    return workspaceActive.value && activeConnectionId.value === connectionId
      ? "active"
      : "inactive";
  }

  return {
    mountedSessionIds,
    runtimeModeFor,
  };
}
