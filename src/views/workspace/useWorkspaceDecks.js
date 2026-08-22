import { computed } from "vue";
import { useTerminalRuntimeDeck } from "../../composables/useTerminalRuntimeDeck";
import { connectionCan, runtimeCan } from "../../utils/connectionCapabilities";

export function useWorkspaceDecks({
  activeConnectionInfo,
  activeTab,
  isDark,
  openSessions,
  preferences,
  sessionRuntime,
}) {
  const canUseSftp = computed(() => connectionCan(activeConnectionInfo.value, "sftp"));
  const activeConnectionId = computed(() => activeConnectionInfo.value?.id || "");
  const shellWorkspaceActive = computed(() => activeTab.value === "shell");
  const sftpWorkspaceActive = computed(() => activeTab.value === "sftp");

  const resolvedTerminalTheme = computed(() => {
    if (!preferences.value.terminalThemeFollowApp) return preferences.value.terminalTheme;
    return isDark.value
      ? preferences.value.terminalThemeDark
      : preferences.value.terminalThemeLight;
  });

  const terminalOptions = computed(() => ({
    terminalFontSize: preferences.value.terminalFontSize,
    terminalFontFamily: preferences.value.terminalFontFamily,
    terminalLineHeight: preferences.value.terminalLineHeight,
    terminalScrollback: preferences.value.terminalScrollback,
    terminalCursorBlink: preferences.value.terminalCursorBlink,
    terminalCursorStyle: preferences.value.terminalCursorStyle,
    terminalCursorInactiveStyle: preferences.value.terminalCursorInactiveStyle,
    terminalCursorWidth: preferences.value.terminalCursorWidth,
    terminalScrollSensitivity: preferences.value.terminalScrollSensitivity,
    terminalFastScrollSensitivity: preferences.value.terminalFastScrollSensitivity,
    terminalSmoothScrollDuration: preferences.value.terminalSmoothScrollDuration,
    terminalAltClickMovesCursor: preferences.value.terminalAltClickMovesCursor,
    terminalRightClickSelectsWord: preferences.value.terminalRightClickSelectsWord,
    terminalScrollOnUserInput: preferences.value.terminalScrollOnUserInput,
    terminalScrollOnEraseInDisplay: preferences.value.terminalScrollOnEraseInDisplay,
    terminalDrawBoldTextInBrightColors: preferences.value.terminalDrawBoldTextInBrightColors,
    terminalMinimumContrastRatio: preferences.value.terminalMinimumContrastRatio,
    terminalCustomGlyphs: preferences.value.terminalCustomGlyphs,
    terminalRescaleOverlappingGlyphs: preferences.value.terminalRescaleOverlappingGlyphs,
    terminalMacOptionIsMeta: preferences.value.terminalMacOptionIsMeta,
    terminalMacOptionClickForcesSelection: preferences.value.terminalMacOptionClickForcesSelection,
    terminalTheme: resolvedTerminalTheme.value,
    terminalWebgl: preferences.value.terminalWebgl,
    terminalTrzsz: preferences.value.terminalTrzsz,
    transferDragUpload: preferences.value.transferDragUpload,
    transferDirectoryUpload: preferences.value.transferDirectoryUpload,
    transferMaxChunkSize: preferences.value.transferMaxChunkSize,
    transferDragInitTimeout: preferences.value.transferDragInitTimeout,
    terminalSearchShortcut: preferences.value.terminalSearchShortcut,
    terminalHighlightSchemes: preferences.value.terminalHighlightSchemes,
  }));

  const terminalDeck = useTerminalRuntimeDeck({
    openSessions,
    activeConnectionId,
    workspaceActive: shellWorkspaceActive,
  });
  const terminalSessions = computed(() =>
    openSessions.value.filter((session) =>
      terminalDeck.mountedSessionIds.value.includes(session.id),
    ),
  );

  const activeSftpConnectionId = computed(() =>
    sftpWorkspaceActive.value && canUseSftp.value ? activeConnectionId.value : "",
  );
  const sftpOpenSessions = computed(() =>
    sftpWorkspaceActive.value
      ? openSessions.value.filter((session) => runtimeCan(sessionRuntime?.(session.id), "sftp"))
      : [],
  );
  const sftpDeck = useTerminalRuntimeDeck({
    openSessions: sftpOpenSessions,
    activeConnectionId: activeSftpConnectionId,
    workspaceActive: sftpWorkspaceActive,
  });
  const sftpSessions = computed(() => {
    if (!sftpWorkspaceActive.value) return [];
    const mountedIds = new Set(sftpDeck.mountedSessionIds.value);
    return sftpOpenSessions.value.filter((session) => mountedIds.has(session.id));
  });

  return {
    activeConnectionId,
    canUseSftp,
    sftpSessions,
    terminalSessions,
    runtimeModeFor: terminalDeck.runtimeModeFor,
    terminalOptions,
  };
}
