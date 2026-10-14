import { computed } from "vue";
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

  // 挂载集合即 openSessions 本身（会话 id 恒为真值），按 id 列表再过滤一遍
  // 只是恒等筛选；sftpOpenSessions 内部已含 sftpWorkspaceActive 门禁。
  const terminalSessions = openSessions;

  const sftpOpenSessions = computed(() =>
    sftpWorkspaceActive.value
      ? openSessions.value.filter((session) => runtimeCan(sessionRuntime?.(session.id), "sftp"))
      : [],
  );
  const sftpSessions = sftpOpenSessions;

  function runtimeModeFor(connectionId) {
    if (!connectionId) return "inactive";
    return shellWorkspaceActive.value && activeConnectionId.value === connectionId
      ? "active"
      : "inactive";
  }

  return {
    activeConnectionId,
    canUseSftp,
    sftpSessions,
    terminalSessions,
    runtimeModeFor,
    terminalOptions,
  };
}
