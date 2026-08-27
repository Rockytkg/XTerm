<script setup>
import "@xterm/xterm/css/xterm.css";
import "../styles/terminal.scss";

import { ImageAddon } from "@xterm/addon-image";
import { ProgressAddon } from "@xterm/addon-progress";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import {
  computed,
  nextTick,
  onActivated,
  onBeforeUnmount,
  onDeactivated,
  onMounted,
  ref,
  watch,
} from "vue";
import { useI18n } from "vue-i18n";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { openExternalUrl } from "../services/appInfo";
import { openContextMenu } from "../services/contextMenu";
import TerminalSearchPopover from "./TerminalSearchPopover.vue";
import { useTerminalDragDrop } from "../composables/useTerminalDragDrop";
import { useTerminalOptionalAddons } from "../composables/useTerminalOptionalAddons";
import { useTerminalSearchPanel } from "../composables/useTerminalSearchPanel";
import { useTerminalSessionRuntime } from "../composables/useTerminalSessionRuntime";
import { registerTerminalOptionWatchers } from "../composables/useTerminalOptionWatchers";
import { createLogger } from "../utils/logger";
import { isPrimaryModifier } from "../utils/platform";
import { getTerminalStatusPalette, getTerminalTheme } from "../utils/terminalColors";
import {
  TerminalHighlightAddon,
  OscNotificationAddon,
  ScriptBridgeAddon,
  TauriClipboardAddon,
  TerminalOutputAddon,
  TerminalResizeAddon,
  TerminalStatusAddon,
  TrzszAddon,
} from "../utils/terminal/addons";
import {
  createRawOutputFrame,
  createRenderedOffsetFrame,
  createResizeFrame,
} from "../utils/terminal/protocol/frames";
import { createRenderedOffsetReporter } from "../utils/terminal/renderedOffsetReporter";
import { createBackgroundSessionSuspender } from "../utils/terminal/backgroundSessionSuspender";
import { createTauriTerminalTransport } from "../utils/terminal/transport/TauriTerminalTransport";
import { createTerminalShortcutHandler } from "../utils/terminal/createTerminalShortcutHandler";
import { createTerminalWheelZoomHandler } from "../utils/terminal/createTerminalWheelZoomHandler";
import { TerminalConnectionPresenter } from "../utils/terminal/TerminalConnectionPresenter";
import { connectionCan } from "../utils/connectionCapabilities";
import {
  classifyTerminalOutputPayload,
  createTerminalOutputByteDecoder,
} from "../utils/terminalOutputPayload";
import { createFrameIntervalSampler } from "../utils/renderingCapabilities";
import { MorePromptCleanup } from "../utils/terminal/addons/pager/MorePromptCleanup";
import {
  TERMINAL_OUTPUT_BACKPRESSURE_HIGH_WATERMARK,
  TERMINAL_OUTPUT_BACKPRESSURE_LOW_WATERMARK,
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
  TERMINAL_OUTPUT_FLUSH_MAX_CHARS,
  TERMINAL_OUTPUT_FLUSH_MS,
  TERMINAL_OUTPUT_WRITE_CHUNK_CHARS,
} from "../utils/terminalPanelHelpers";
import { binaryStringToBase64, shouldSendBackspaceAsBs } from "../utils/terminal/inputRouting";
import { compileTerminalHighlightRules } from "../utils/terminal/highlightRules";
import { createXtermOptions } from "../utils/terminal/xtermOptions";
import { createTerminalContextMenuItems } from "../utils/terminal/contextMenu";
import { publishTerminalOutput, registerScriptBridge } from "../services/scripting/bridges";

const props = defineProps({
  activeConnection: { type: Object, default: null },
  connectionState: {
    type: Object,
    default: () => ({ status: "idle", phase: null, error: null }),
  },
  sessionId: { type: String, default: "" },
  visible: { type: Boolean, default: true },
  runtimeMode: { type: String, default: "active" },
  recordingActive: { type: Boolean, default: false },
  terminalFontSize: { type: Number, default: 16 },
  terminalFontFamily: { type: String, default: "Cascadia Code" },
  terminalLineHeight: { type: Number, default: 1 },
  terminalScrollback: { type: Number, default: 9001 },
  terminalCursorBlink: { type: Boolean, default: true },
  terminalCursorStyle: { type: String, default: "block" },
  terminalCursorInactiveStyle: { type: String, default: "outline" },
  terminalCursorWidth: { type: Number, default: 1 },
  terminalScrollSensitivity: { type: Number, default: 1 },
  terminalFastScrollSensitivity: { type: Number, default: 5 },
  terminalSmoothScrollDuration: { type: Number, default: 0 },
  terminalAltClickMovesCursor: { type: Boolean, default: true },
  terminalRightClickSelectsWord: { type: Boolean, default: false },
  terminalScrollOnUserInput: { type: Boolean, default: true },
  terminalScrollOnEraseInDisplay: { type: Boolean, default: false },
  terminalDrawBoldTextInBrightColors: { type: Boolean, default: false },
  terminalMinimumContrastRatio: { type: Number, default: 1 },
  terminalCustomGlyphs: { type: Boolean, default: true },
  terminalRescaleOverlappingGlyphs: { type: Boolean, default: false },
  terminalMacOptionIsMeta: { type: Boolean, default: false },
  terminalMacOptionClickForcesSelection: { type: Boolean, default: false },
  terminalTheme: { type: String, default: "default" },
  terminalWebgl: { type: Boolean, default: true },
  terminalTrzsz: { type: Boolean, default: true },
  transferDragUpload: { type: Boolean, default: true },
  transferDirectoryUpload: { type: Boolean, default: true },
  transferMaxChunkSize: { type: Number, default: 10 * 1024 * 1024 },
  transferDragInitTimeout: { type: Number, default: 3000 },
  terminalSearchShortcut: { type: String, default: "Ctrl+F" },
  searchOpenToken: { type: Number, default: 0 },
  terminalHighlightSchemes: { type: Array, default: () => [] },
});

const emit = defineEmits([
  "terminalFontSizeChange",
  "terminalRecordChunk",
  "terminalReady",
  "retryConnection",
  "terminalResize",
]);
const { t, te, locale } = useI18n();
const { sessionRegistry } = useWorkspaceStore();
const logger = createLogger("frontend.terminal.panel");
const terminalMount = ref(null);

let terminal;
let searchAddon;
let searchResultsDisposable;
let progressAddon;
let imageAddon;
let clipboardAddon;
let oscNotificationAddon;
let webLinksAddon;
let unicode11Addon;
let trzszAddon;
let highlightAddon;
let disposed = false;
let routeViewActive = true;
let setupGeneration = 0;
let terminalOutputCursor = 0;
let terminalOutputCursorSessionId = "";
let sessionRuntimeController;
let terminalSessionRuntime;
let preserveViewportForNextBackendSession = false;
let terminalPayloadQueue = Promise.resolve();
let unregisterScriptBridge = null;
const frameIntervalSampler = createFrameIntervalSampler();
const terminalOutputByteDecoder = createTerminalOutputByteDecoder();

function refreshTerminalViewport() {
  if (!terminal) return;
  terminal.refresh(0, Math.max(0, terminal.rows - 1));
}

function isForegroundRuntime() {
  return routeViewActive && props.visible && props.runtimeMode === "active";
}

const terminalBg = computed(() => getTerminalTheme(props.terminalTheme).background);
const terminalSurfaceStyle = computed(() => ({
  "--terminal-active-background": terminalBg.value,
}));
const terminalResizeAddon = new TerminalResizeAddon({
  getMount: () => terminalMount.value,
  getSessionId: () => props.sessionId,
  onFrontendResize: (size) => {
    emit("terminalResize", size);
  },
  onBackendResize: ({ sessionId, cols, rows, widthPx, heightPx }) => {
    const channel = sessionRuntimeController?.currentChannel();
    if (!channel || !isForegroundRuntime() || (sessionId && channel.sessionId !== sessionId)) {
      return false;
    }
    terminalTransport
      .send(
        createResizeFrame({
          sessionId,
          channelId: channel.channelId,
          cols,
          rows,
          widthPx,
          heightPx,
        }),
      )
      .catch((error) => {
        logger.error("resize.failed", error);
      });
    return true;
  },
  canSyncBackend: () => !!sessionRuntimeController?.canSyncBackend(),
  isEnabled: () => isForegroundRuntime(),
  isDisposed: () => disposed,
});
const terminalHighlightAddon = new TerminalHighlightAddon({
  getEnabled: () => !!props.activeConnection && isForegroundRuntime(),
});
const terminalTransport = createTauriTerminalTransport({ logger });

const activeHighlightScheme = computed(() => {
  if (!props.activeConnection || props.activeConnection.terminalHighlightEnabled === false)
    return null;
  return (
    props.terminalHighlightSchemes.find((scheme) => scheme.themes?.includes(props.terminalTheme)) ||
    null
  );
});

const compiledHighlightRules = computed(() => {
  return compileTerminalHighlightRules(activeHighlightScheme.value?.rules, logger);
});

const connectionFailureLabel = computed(() => {
  const error = props.connectionState?.error;
  if (!error) return "";

  const code = error.code || "unknown";
  const key = `connectionErrors.${code}`;
  return te(key) ? t(key, error.args || {}) : error.detail || t("connectionErrors.unknown");
});

const connectionFailureDetail = computed(() => {
  const detail = props.connectionState?.error?.detail || props.connectionState?.statusDetail;
  return typeof detail === "string" ? detail.trim() : "";
});

const connectionStatusDetail = computed(() => {
  const detail = props.connectionState?.statusDetail;
  return typeof detail === "string" ? detail.trim() : "";
});

const transferMessages = computed(() => {
  const currentLocale = locale.value;
  return {
    currentLocale,
    chooseUploadTitle: t("terminal.transfer.chooseUploadTitle"),
    chooseUploadDirectoryTitle: t("terminal.transfer.chooseUploadDirectoryTitle"),
    chooseDownloadDirectoryTitle: t("terminal.transfer.chooseDownloadDirectoryTitle"),
    allFilesLabel: t("terminal.transfer.allFilesLabel"),
    formatSavedFiles: (fileNames, destination) => {
      const suffix = fileNames.length > 1 ? "Many" : "One";
      const key = destination
        ? `terminal.transfer.saved${suffix}To`
        : `terminal.transfer.saved${suffix}`;
      const label = t(key, { count: fileNames.length, destination });
      return [label, ...fileNames].join("\r\n- ");
    },
  };
});

const isDisconnectedState = computed(() =>
  ["closed", "failed"].includes(props.connectionState?.status),
);

const {
  closeSearchPanel,
  openSearchPanel,
  resetSearchState,
  runSearch,
  searchEmpty,
  searchOpen,
  searchResultLabel,
  searchTerm,
  setSearchResults,
} = useTerminalSearchPanel({
  props,
  t,
  focusTerminal: () => terminal?.focus(),
  isForegroundRuntime,
  getSearchAddon: () => searchAddon,
});

const terminalDragDrop = useTerminalDragDrop({
  logger,
  shouldListen: () => props.transferDragUpload && props.terminalTrzsz,
  handleDrop: (paths) => {
    trzszAddon?.uploadPaths?.(paths).catch((error) => {
      logger.warn("trzsz.drag_upload.failed", error);
    });
  },
});

const terminalOptionalAddons = useTerminalOptionalAddons({
  logger,
  getContext: () => ({
    terminal,
    disposed,
    generation: setupGeneration,
    isForegroundRuntime: isForegroundRuntime(),
    terminalWebgl: props.terminalWebgl,
  }),
  loadAddon: (label, createAddon, assignAddon) =>
    loadTerminalAddon(label, createAddon, assignAddon),
});

function refitTerminalAfterFontMetricsChange() {
  if (!props.visible) return;
  refreshTerminalViewport();
  nextTick(() => terminalResizeAddon.scheduleFontMetricsRefit());
}

function applyRuntimePresentation() {
  if (!terminal) return;

  const cursorBlink = props.terminalCursorBlink && isForegroundRuntime();
  if (terminal.options.cursorBlink !== cursorBlink) terminal.options.cursorBlink = cursorBlink;
  highlightAddon?.setRules(compiledHighlightRules.value);

  if (isForegroundRuntime()) {
    terminalOptionalAddons.syncTerminalRenderer(setupGeneration);
    nextTick(() => {
      terminalResizeAddon.scheduleFit({ immediate: true });
      refreshTerminalViewport();
      terminal?.focus();
    });
    return;
  }

  resetSearchState();
  terminalOptionalAddons.syncTerminalRenderer(setupGeneration);
  terminal.blur?.();
}

function resetTerminalOutputCursor() {
  terminalOutputCursor = 0;
  terminalOutputCursorSessionId = "";
  terminalPayloadQueue = Promise.resolve();
  terminalOutputByteDecoder.reset();
  renderedOffsetReporter.reset();
  sessionRuntimeController.resetOutputRouting();
}

function terminalOutputPayloadState() {
  return sessionRuntimeController.outputPayloadState({
    terminalOutputCursor,
    terminalOutputCursorSessionId,
  });
}

function rememberTerminalOffset(endOffset) {
  terminalOutputCursor = endOffset;
  terminalOutputCursorSessionId = props.sessionId || "";
}

function queueBackendInput(data) {
  sessionRuntimeController.queueText(data);
}

function queueBackendBytes(dataBase64) {
  sessionRuntimeController.queueBytes(dataBase64);
}

function currentTerminalLineText() {
  const buffer = terminal?.buffer?.active;
  if (!buffer) return "";
  const line = buffer.getLine(buffer.baseY + buffer.cursorY);
  return line?.translateToString(false) || "";
}

function clearMorePromptForContinueInput(data) {
  morePromptCleanup.observeInput(data);
}

function forwardTerminalInput(data) {
  if (!props.sessionId) return;
  if (trzszAddon?.processTerminalInput(data)) return;
  clearMorePromptForContinueInput(data);
  queueBackendInput(data);
}

function handleBackspace(event) {
  if (!shouldSendBackspaceAsBs(event, props.activeConnection, props.sessionId)) {
    return true;
  }
  event.preventDefault();
  event.stopPropagation();
  queueBackendInput("\x08");
  return false;
}

const renderedOffsetReporter = createRenderedOffsetReporter({
  send: (offset) => {
    const channel = sessionRuntimeController?.currentChannel();
    if (!channel || (props.sessionId && channel.sessionId !== props.sessionId)) return;
    terminalTransport
      .send(
        createRenderedOffsetFrame({
          sessionId: channel.sessionId,
          channelId: channel.channelId,
          offset,
        }),
      )
      .catch((error) => {
        logger.warn("rendered_offset.report.failed", error);
      });
  },
});

const terminalOutputAddon = new TerminalOutputAddon({
  onRecordChunk: (chunk) => emit("terminalRecordChunk", chunk),
  onBackpressureResume: () => renderedOffsetReporter.onBackpressureResume(),
  isDisposed: () => disposed,
  isRecordingActive: () => props.recordingActive,
  flushDelay: TERMINAL_OUTPUT_FLUSH_MS,
  getFrameBudgetMs: () => frameIntervalSampler.currentFrameIntervalMs(),
  maxChars: TERMINAL_OUTPUT_FLUSH_MAX_CHARS,
  writeChunkChars: TERMINAL_OUTPUT_WRITE_CHUNK_CHARS,
  highWatermark: TERMINAL_OUTPUT_BACKPRESSURE_HIGH_WATERMARK,
  lowWatermark: TERMINAL_OUTPUT_BACKPRESSURE_LOW_WATERMARK,
});

const morePromptCleanup = new MorePromptCleanup({
  isEnabled: () => props.activeConnection?.terminalMorePromptCleanup === true,
  readCurrentLine: () => currentTerminalLineText(),
  queueLocalClear: (data) => terminalOutputAddon.queue(data, { immediate: true }),
});

// 脚本桥接：脚本引擎的输入/屏幕/输出能力都经这个 xterm 插件暴露。
const scriptBridgeAddon = new ScriptBridgeAddon();

const terminalStatusAddon = new TerminalStatusAddon({
  getConnection: () => props.activeConnection,
  getPalette: () => getTerminalStatusPalette(props.terminalTheme),
  getFailureLabel: () => connectionFailureLabel.value,
  getFailureDetail: () => connectionFailureDetail.value,
  getStatusDetail: () => connectionStatusDetail.value,
  queueWrite: (...args) => terminalOutputAddon.queue(...args),
  t,
});

const connectionPresenter = new TerminalConnectionPresenter({
  dropOutput: () => terminalOutputAddon.drop(),
  getState: () => props.connectionState,
  handleStatus: (status, phase) => sessionRuntimeController?.handleConnectionStatus(status, phase),
  onConnecting: () => void signalTerminalReadyAfterPaint(),
  resetStatus: () => terminalStatusAddon.reset(),
});

terminalSessionRuntime = useTerminalSessionRuntime({
  drainOutput: () =>
    terminalPayloadQueue.catch(() => {}).then(() => terminalOutputAddon.waitForFlush()),
  dropOutput: () => connectionPresenter.reset(),
  getContext: () => ({
    sessionId: props.sessionId || "",
    connectionId: props.activeConnection?.connectionId || props.activeConnection?.id || "",
    connectionStatus: props.connectionState?.status || "idle",
    connectionPhase: props.connectionState?.phase || null,
    capabilities: props.activeConnection?.capabilities || null,
    disposed,
    hasActiveConnection: !!props.activeConnection,
    isForeground: isForegroundRuntime(),
    terminalReady: !!terminal,
  }),
  logger,
  onSessionData: (payload) => {
    enqueueTerminalPayload(payload);
  },
  queueResizeSync: () => terminalResizeAddon.queueBackendSync(null, { immediate: true }),
  releaseStatus: () => terminalStatusAddon.release(),
  setActiveSessionChannel: (connectionId, channelId) => {
    const frontendSessionId = props.activeConnection?.id || "";
    if (frontendSessionId) sessionRegistry.setActiveSessionChannel(frontendSessionId, channelId);
  },
  writeStatus: (status) => terminalStatusAddon.write(status),
  transport: terminalTransport,
});
sessionRuntimeController = terminalSessionRuntime.controller;

// 后台 30s 挂起（与 WebGL dispose 同节奏）：detach 后输出链路停止，
// 会话录音自然暂停（无数据即无 chunk），恢复时增量追平的 chunk 照常录制。
// presentation gate 只在首绘路径生效，挂起/恢复不经过它，无需特殊处理。
const backgroundSuspender = createBackgroundSessionSuspender({
  isBackground: () => !isForegroundRuntime(),
  suspend: () => void sessionRuntimeController.suspendForBackground(),
  resume: () => void sessionRuntimeController.resumeFromBackground(),
});

const handleTerminalShortcut = createTerminalShortcutHandler({
  copySelection: () => {
    void copyTerminalSelection();
  },
  hasSelection: () => !!terminal?.hasSelection?.(),
  pasteClipboard: () => {
    void pasteClipboardIntoTerminal();
  },
  sendInterrupt: () => {
    forwardTerminalInput("\x03");
  },
  canOpenSearch: () => !!searchAddon && isForegroundRuntime(),
  openSearch: openSearchPanel,
  searchShortcut: () => props.terminalSearchShortcut,
});

const handleTerminalWheel = createTerminalWheelZoomHandler({
  getFontSize: () => Number(props.terminalFontSize || 16),
  setFontSize: (fontSize) => emit("terminalFontSizeChange", fontSize),
  minFontSize: TERMINAL_FONT_SIZE_MIN,
  maxFontSize: TERMINAL_FONT_SIZE_MAX,
});

function disposeTerminalAddons() {
  searchResultsDisposable?.dispose();
  searchResultsDisposable = null;
  searchAddon?.dispose?.();
  searchAddon = null;
  progressAddon?.dispose?.();
  progressAddon = null;
  imageAddon?.dispose?.();
  imageAddon = null;
  clipboardAddon?.dispose?.();
  clipboardAddon = null;
  oscNotificationAddon?.dispose?.();
  oscNotificationAddon = null;
  webLinksAddon?.dispose?.();
  webLinksAddon = null;
  unicode11Addon?.dispose?.();
  unicode11Addon = null;
  trzszAddon?.dispose?.();
  trzszAddon = null;
  highlightAddon?.dispose?.();
  highlightAddon = null;
  terminalOptionalAddons.disposeOptionalAddons();
}

function loadTerminalAddon(label, createAddon, assignAddon) {
  if (!terminal) return null;
  try {
    const addon = createAddon();
    terminal.loadAddon(addon);
    assignAddon(addon);
    return addon;
  } catch (error) {
    logger.warn("addon.load.failed", { label }, error);
    assignAddon(null);
    return null;
  }
}

function installStableTerminalAddons() {
  if (!terminal) return;

  clipboardAddon = loadTerminalAddon(
    "clipboard",
    () => new TauriClipboardAddon(),
    (addon) => {
      clipboardAddon = addon;
    },
  );
  searchAddon = loadTerminalAddon(
    "search",
    () => new SearchAddon(),
    (addon) => {
      searchAddon = addon;
    },
  );
  if (searchAddon) {
    searchResultsDisposable = searchAddon.onDidChangeResults((result) => {
      setSearchResults(result);
    });
  }
  progressAddon = loadTerminalAddon(
    "progress",
    () => new ProgressAddon(),
    (addon) => {
      progressAddon = addon;
    },
  );
  imageAddon = loadTerminalAddon(
    "image",
    () =>
      new ImageAddon({
        enableSizeReports: true,
      }),
    (addon) => {
      imageAddon = addon;
    },
  );
  oscNotificationAddon = loadTerminalAddon(
    "osc-notifications",
    () => new OscNotificationAddon(),
    (addon) => {
      oscNotificationAddon = addon;
    },
  );
  unicode11Addon = loadTerminalAddon(
    "unicode11",
    () => new Unicode11Addon(),
    (addon) => {
      unicode11Addon = addon;
    },
  );
  if (terminal.unicode) {
    terminal.unicode.activeVersion = "11";
  }
  webLinksAddon = loadTerminalAddon(
    "web-links",
    () =>
      new WebLinksAddon(async (event, uri) => {
        if (!isPrimaryModifier(event)) return;
        await openExternalUrl(uri);
      }),
    (addon) => {
      webLinksAddon = addon;
    },
  );
  loadTerminalAddon(
    "output",
    () => terminalOutputAddon,
    () => {},
  );
  loadTerminalAddon(
    "script-bridge",
    () => scriptBridgeAddon,
    () => {},
  );
  loadTerminalAddon(
    "status",
    () => terminalStatusAddon,
    () => {},
  );
  loadTerminalAddon(
    "resize",
    () => terminalResizeAddon,
    () => {},
  );
  highlightAddon = loadTerminalAddon(
    "highlight",
    () => terminalHighlightAddon,
    (addon) => {
      highlightAddon = addon;
    },
  );
  trzszAddon = loadTerminalAddon(
    "trzsz",
    () =>
      new TrzszAddon({
        getSessionContext: () => {
          const channel = sessionRuntimeController.currentChannel();
          return {
            active: !!channel && channel.sessionId === props.sessionId && isForegroundRuntime(),
            sessionId: channel?.sessionId || props.sessionId,
            channelId: channel?.channelId ?? null,
            capabilities: props.activeConnection?.capabilities || null,
          };
        },
        sendText: queueBackendInput,
        sendBytes: queueBackendBytes,
        setRawOutput: ({ enabled }) => {
          const channel = sessionRuntimeController.currentChannel();
          if (
            !connectionCan(props.activeConnection, "rawOutput") ||
            !channel ||
            channel.sessionId !== props.sessionId
          ) {
            return Promise.resolve();
          }
          return terminalTransport.send(
            createRawOutputFrame({
              sessionId: channel.sessionId,
              channelId: channel.channelId,
              enabled,
            }),
          );
        },
        enabled: props.terminalTrzsz,
        directoryUpload: props.transferDirectoryUpload,
        dragInitTimeout: props.transferDragInitTimeout,
        maxDataChunkSize: props.transferMaxChunkSize,
        messages: transferMessages.value,
        writeTerminal: (data, options = {}) => terminalOutputAddon.queue(data, options),
      }),
    (addon) => {
      trzszAddon = addon;
    },
  );
}

function setupTerminal() {
  if (!terminalMount.value || !props.activeConnection) {
    logger.warn("setup.skipped");
    return;
  }

  const generation = ++setupGeneration;
  morePromptCleanup.reset();
  void sessionRuntimeController.deactivate();
  terminalMount.value?.removeEventListener("wheel", handleTerminalWheel, { capture: true });
  terminal?.dispose();
  terminal = null;
  disposeTerminalAddons();
  terminalResizeAddon.reset();
  if (
    disposed ||
    generation !== setupGeneration ||
    !terminalMount.value ||
    !props.activeConnection
  ) {
    logger.warn("setup.aborted", { generation });
    return;
  }

  terminal = new Terminal(createXtermOptions(props, isForegroundRuntime()));
  installStableTerminalAddons();
  terminal.open(terminalMount.value);
  terminalOptionalAddons.schedulePostOpenTerminalAddons(generation);
  terminalMount.value.addEventListener("wheel", handleTerminalWheel, {
    passive: false,
    capture: true,
  });
  terminalOptionalAddons.syncTerminalRenderer(generation);
  if (isForegroundRuntime()) {
    terminalResizeAddon.fitIfNeeded();
  }
  syncTerminalRuntimeResources();
  replayConnectionStatus();

  terminal.attachCustomKeyEventHandler((event) => {
    if (!handleBackspace(event)) return false;
    return handleTerminalShortcut(event);
  });

  terminal.onData((data) => {
    if (isDisconnectedState.value && data === "\r") {
      resetTerminalViewportState();
      emit("retryConnection");
      return;
    }

    if (props.sessionId) {
      forwardTerminalInput(data);
    }
  });
  terminal.onBinary((data) => {
    if (!props.sessionId) return;
    if (trzszAddon?.processBinaryInput(data)) return;
    queueBackendBytes(binaryStringToBase64(data));
  });
  terminalResizeAddon.observe();
}

function replayConnectionStatus() {
  connectionPresenter.replay();
}

async function signalTerminalReadyAfterPaint() {
  const generation = setupGeneration;
  const sessionId = props.activeConnection?.id || "";
  if (!terminal || !sessionId) return;
  await terminalOutputAddon.waitForFlush();
  await new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
  if (
    disposed ||
    generation !== setupGeneration ||
    !terminal ||
    props.activeConnection?.id !== sessionId ||
    props.connectionState?.status !== "connecting"
  ) {
    return;
  }
  emit("terminalReady");
}

function invalidateHighlightCache() {
  highlightAddon?.invalidate();
}

async function copyTerminalSelection() {
  if (await clipboardAddon?.copySelection?.()) terminal?.clearSelection();
}

async function pasteClipboardIntoTerminal() {
  if (!terminal || !props.sessionId || !clipboardAddon?.isSupported?.()) return;
  await clipboardAddon.pasteIntoTerminal();
}

function selectAllTerminal() {
  terminal?.selectAll();
  terminal?.focus();
}

function clearTerminalOutput() {
  if (!terminal) return;
  terminalStatusAddon.clear();
  terminal.clear();
  invalidateHighlightCache();
  terminal.focus();
}

function resetTerminalViewportState() {
  if (!terminal) return;
  morePromptCleanup.reset();
  connectionPresenter.reset();
  resetTerminalOutputCursor();
  terminal.reset();
  invalidateHighlightCache();
}

function resetBackendSessionState({ preserveViewport = false } = {}) {
  morePromptCleanup.reset();
  connectionPresenter.resetBackendSession({ preserveViewport });
  resetTerminalOutputCursor();
  if (!preserveViewport) {
    invalidateHighlightCache();
    terminal?.reset();
  }
}

async function provideTerminalContextMenu(event) {
  if (!terminal) return;
  const selectedText = terminal.getSelection();
  const hasSelection = !!selectedText;
  await openContextMenu(event, {
    suppressDefaultEditItems: true,
    items: createTerminalContextMenuItems({
      t,
      hasSelection,
      canPaste: !!props.sessionId && !!clipboardAddon?.isSupported?.(),
      hasSearch: !!searchAddon,
      copySelection: copyTerminalSelection,
      pasteClipboard: pasteClipboardIntoTerminal,
      selectAll: selectAllTerminal,
      clearOutput: clearTerminalOutput,
      openSearch: openSearchPanel,
    }),
  });
}

watch(compiledHighlightRules, () => {
  if (!terminal) return;
  highlightAddon?.setRules(compiledHighlightRules.value);
});

registerTerminalOptionWatchers({
  getTerminal: () => terminal,
  getTheme: () => getTerminalTheme(props.terminalTheme),
  props,
  refitTerminalAfterFontMetricsChange,
  refreshTerminalViewport,
  syncTerminalRenderer: () => terminalOptionalAddons.syncTerminalRenderer(setupGeneration),
  isForegroundRuntime,
});

watch(
  () => [props.visible, props.runtimeMode],
  () => {
    syncTerminalRuntimeResources();
  },
);

watch(
  () => props.terminalTrzsz,
  () => {
    trzszAddon?.setEnabled(props.terminalTrzsz);
  },
);

// shouldListen 只在 attach 时判断一次，运行中切换该偏好需要立即摘挂拖放监听
watch(
  () => props.transferDragUpload,
  (enabled) => {
    if (enabled) {
      void terminalDragDrop.attachDragDropListener();
    } else {
      terminalDragDrop.detachDragDropListener();
    }
  },
);

watch(
  () => locale.value,
  () => {
    trzszAddon?.setMessages(transferMessages.value);
    replayConnectionStatus();
  },
);

watch(
  () => [props.transferDirectoryUpload, props.transferDragInitTimeout, props.transferMaxChunkSize],
  () => {
    trzszAddon?.setOptions?.({
      directoryUpload: props.transferDirectoryUpload,
      dragInitTimeout: props.transferDragInitTimeout,
      maxDataChunkSize: props.transferMaxChunkSize,
    });
  },
);

watch(
  () => props.sessionId,
  (sessionId, previousSessionId) => {
    void sessionRuntimeController.deactivate();
    resetTerminalOutputCursor();
    if (!sessionId) {
      if (terminal && previousSessionId) {
        resetBackendSessionState({ preserveViewport: true });
        preserveViewportForNextBackendSession = true;
      }
      terminalResizeAddon.resetBackendSyncState();
      return;
    }
    if (terminal) {
      if (sessionId !== previousSessionId) {
        const preserveViewport = !previousSessionId || preserveViewportForNextBackendSession;
        preserveViewportForNextBackendSession = false;
        resetBackendSessionState({ preserveViewport });
      }
      syncTerminalRuntimeResources();
    }
  },
);

watch(
  () => [
    props.connectionState?.status,
    props.connectionState?.phase,
    props.connectionState?.statusDetail,
    props.connectionState?.error?.code,
    props.connectionState?.error?.message,
    props.connectionState?.error?.detail,
  ],
  () => {
    replayConnectionStatus();
  },
  { immediate: true },
);

function enqueueTerminalPayload(payload) {
  terminalPayloadQueue = terminalPayloadQueue
    .catch(() => {})
    .then(() => handleTerminalData(payload));
}

async function handleTerminalData(payload) {
  if (!payload?.kind || !terminal) return;
  const payloadState = terminalOutputPayloadState();
  const decision = classifyTerminalOutputPayload(payload, payloadState);
  if (decision.kind === "ignore") return;
  const { normalized } = decision;

  if (trzszAddon?.processServerOutput({ ...payload, ...normalized })) {
    rememberTerminalOffset(normalized.endOffset);
    renderedOffsetReporter.noteConsumed(
      (normalized.dataBase64 || normalized.data || "").length,
      normalized.endOffset,
    );
    return;
  }
  terminalStatusAddon.release();
  // raw 路径唯一一次 base64 解码：classify 阶段无裁剪时已透传原串，且必须用
  // 跨包流式 TextDecoder 保留多字节字符，不能合并进 classify 的纯函数里。
  const rawData =
    decision.kind === "raw"
      ? terminalOutputByteDecoder.decode(normalized.dataBase64, payload.encoding)
      : normalized.data;
  if (decision.kind === "text") terminalOutputByteDecoder.reset();
  const data = morePromptCleanup.cleanOutput(rawData);
  if (data) {
    terminalOutputAddon.queue(data, {
      immediate: connectionCan(props.activeConnection, "serialBaudDetection"),
      recordable: true,
    });
    // 同步发布给脚本引擎（无订阅者时是零成本短路）。
    publishTerminalOutput(props.activeConnection?.id || "", data);
  }
  rememberTerminalOffset(normalized.endOffset);
  renderedOffsetReporter.noteConsumed(data.length, normalized.endOffset);
}

function syncTerminalRuntimeResources() {
  applyRuntimePresentation();
  backgroundSuspender.sync(isForegroundRuntime());
  if (isForegroundRuntime()) {
    frameIntervalSampler.start();
  } else {
    frameIntervalSampler.stop();
  }
  terminalResizeAddon.observe();
  sessionRuntimeController.syncRuntimeResources();
}

onMounted(() => {
  disposed = false;
  routeViewActive = true;
  void terminalDragDrop.attachDragDropListener();
  unregisterScriptBridge = registerScriptBridge(
    props.activeConnection?.id || "",
    scriptBridgeAddon,
  );
  setupTerminal();
});

onActivated(() => {
  routeViewActive = true;
  void terminalDragDrop.attachDragDropListener();
  syncTerminalRuntimeResources();
});

onDeactivated(() => {
  routeViewActive = false;
  trzszAddon?.stopTransfer?.();
  terminalDragDrop.detachDragDropListener();
  syncTerminalRuntimeResources();
});

onBeforeUnmount(() => {
  disposed = true;
  unregisterScriptBridge?.();
  unregisterScriptBridge = null;
  scriptBridgeAddon.dispose();
  backgroundSuspender.dispose();
  frameIntervalSampler.stop();
  terminalDragDrop.disposeDragDropListener();
  void sessionRuntimeController.deactivate();
  terminalOutputAddon.flush();
  terminalOutputAddon.dispose();
  terminalResizeAddon.dispose();
  terminalSessionRuntime?.dispose();
  terminalMount.value?.removeEventListener("wheel", handleTerminalWheel, { capture: true });
  disposeTerminalAddons();
  terminal?.dispose();
});
</script>

<template>
  <article
    class="terminal-workspace flex flex-1 flex-col overflow-hidden"
    @contextmenu="provideTerminalContextMenu"
  >
    <section
      class="terminal-surface min-h-0 flex-1 overflow-hidden"
      :style="terminalSurfaceStyle"
    >
      <TerminalSearchPopover
        v-if="searchOpen"
        v-model:term="searchTerm"
        :result-label="searchResultLabel"
        :is-empty="searchEmpty"
        @run="runSearch"
        @close="closeSearchPanel"
      />
      <div
        ref="terminalMount"
        class="terminal-mount ui-fill-block ui-fill-inline"
      />
    </section>
  </article>
</template>
