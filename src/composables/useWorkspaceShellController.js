import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import {
  Activity,
  FileCode,
  FileText,
  HardDrive,
  Info,
  RadioTower,
  Shield,
  RefreshCw,
  Search,
  SquareTerminal,
} from "@lucide/vue";
import { useToasts } from "./useToasts";
import { useWorkspaceUiStore } from "../stores/workspaceUiStore";
import { createRafThrottle } from "../utils/schedulers";
import { createLogger } from "../utils/logger";
import { runViewTransition } from "../utils/motion";
import { connectionCan } from "../utils/connectionCapabilities";
import { openScriptRunPicker } from "../services/scripting/scriptRunPicker";

const RIGHT_SIDEBAR_MIN_WIDTH = 280;
const RIGHT_SIDEBAR_MAX_WIDTH = 640;
const RIGHT_SIDEBAR_DEFAULT_WIDTH = 336;
const SERVICE_SIDEBAR_VIEWS = new Set(["proxy", "file-service"]);
const WORKSPACE_SIDEBAR_TRANSITION_CLASS = "workspace-sidebar-transition-running";
const logger = createLogger("frontend.workspace.shell");

export function useWorkspaceShellController({
  activeConnection,
  activeConnectionInfo,
  activeTab,
  lastSerialBaudEvent,
  preferences,
  sessionRecordings,
  sessionTabs,
  connectTo,
  reconnectSerialAutoBaud,
  refreshConnectionList,
  selectTab,
  toggleSessionRecording,
  navigate,
}) {
  const { t } = useI18n();
  const { showToast, updateToast } = useToasts();
  const workspaceUi = useWorkspaceUiStore();
  const { rightSidebarView, terminalSearchOpenToken } = storeToRefs(workspaceUi);

  const pendingSerialRedetect = ref(null);
  const isShellTab = computed(() => activeTab.value === "shell");
  const rightSidebarOpen = computed(() => {
    const view = rightSidebarView.value;
    if (!isShellTab.value || !view) return false;
    return SERVICE_SIDEBAR_VIEWS.has(view) || sessionTabs.value.length > 0;
  });

  const rightSidebarWidth = ref(RIGHT_SIDEBAR_DEFAULT_WIDTH);

  const navItems = computed(() => [
    { id: "shell", icon: SquareTerminal, labelKey: "nav.terminal" },
    ...(connectionCan(activeConnectionInfo.value, "sftp")
      ? [{ id: "sftp", icon: HardDrive, labelKey: "nav.sftp" }]
      : []),
  ]);

  const tabbarSideButtons = computed(() => {
    if (!isShellTab.value) return [];

    const hasSession = sessionTabs.value.length > 0;
    const hasMetrics = connectionCan(activeConnectionInfo.value, "metrics");
    const canSearch = !!activeConnectionInfo.value;
    const canRedetectBaud =
      connectionCan(activeConnectionInfo.value, "serialBaudDetection") &&
      activeConnectionInfo.value?.baudRate === "auto";
    return [
      {
        id: "panel-session",
        icon: Info,
        label: t("sidebar.views.session"),
        active: rightSidebarOpen.value && rightSidebarView.value === "session",
        disabled: !hasSession,
      },
      ...(hasMetrics
        ? [
            {
              id: "panel-performance",
              icon: Activity,
              label: t("sidebar.views.performance"),
              active: rightSidebarOpen.value && rightSidebarView.value === "performance",
              disabled: !hasSession,
            },
          ]
        : []),
      ...(preferences.value.proxyToolbarEnabled
        ? [
            {
              id: "panel-proxy",
              icon: Shield,
              label: t("sidebar.views.proxy"),
              active: rightSidebarOpen.value && rightSidebarView.value === "proxy",
              disabled: false,
            },
          ]
        : []),
      ...(preferences.value.fileServiceToolbarEnabled !== false
        ? [
            {
              id: "panel-file-service",
              icon: RadioTower,
              label: t("sidebar.views.fileService"),
              active: rightSidebarOpen.value && rightSidebarView.value === "file-service",
              disabled: false,
            },
          ]
        : []),
      {
        id: "quick-search",
        icon: Search,
        label: t("sidebar.actions.search"),
        active: false,
        disabled: !canSearch,
      },
      {
        id: "quick-run-script",
        icon: FileCode,
        label: t("scripts.runScript"),
        active: false,
        disabled: !activeConnectionInfo.value,
      },
      {
        id: "quick-recording",
        icon: FileText,
        label: sessionRecordings.value.get(activeConnection.value)?.active
          ? t("overview.session.stopRecording")
          : t("overview.session.startRecording"),
        active: !!sessionRecordings.value.get(activeConnection.value)?.active,
        disabled: !activeConnectionInfo.value,
      },
      ...(canRedetectBaud
        ? [
            {
              id: "quick-redetect-baud",
              icon: RefreshCw,
              label: t("overview.session.redetectBaud"),
              active: false,
              disabled: false,
            },
          ]
        : []),
    ];
  });

  watch(
    () => sessionTabs.value.length,
    (sessionCount) => {
      if (sessionCount === 0 && !SERVICE_SIDEBAR_VIEWS.has(rightSidebarView.value)) {
        rightSidebarView.value = null;
      }
    },
    { immediate: true },
  );

  watch(
    () => activeConnectionInfo.value?.capabilities,
    () => {
      if (!connectionCan(activeConnectionInfo.value, "sftp") && activeTab.value === "sftp") {
        selectTab("shell");
      }
      if (
        !connectionCan(activeConnectionInfo.value, "metrics") &&
        rightSidebarView.value === "performance"
      ) {
        rightSidebarView.value = "session";
      }
    },
  );

  watch(
    () => lastSerialBaudEvent.value,
    (event) => {
      const pending = pendingSerialRedetect.value;
      if (!pending || !event || pending.connectionId !== event.connectionId) return;

      if (event.error) {
        updateToast(pending.toastId, {
          type: "error",
          title: t("notifications.serialBaudFailed"),
          message: event.error,
        });
      } else if (event.confirmed) {
        updateToast(pending.toastId, {
          type: "success",
          title: t("notifications.serialBaudSucceeded"),
          message: "",
        });
      } else {
        updateToast(pending.toastId, {
          type: "warning",
          title: t("notifications.serialBaudUnconfirmed"),
          message: t("notifications.serialBaudUnconfirmedDesc", { baud: event.baudRate }),
        });
      }
      pendingSerialRedetect.value = null;
    },
  );

  function onConnectTo(id) {
    logger.info("connection.open.requested", { connectionId: id });
    connectTo(id, { preserveActiveTab: activeTab.value === "sftp" });
    navigate("workspace");
  }

  async function onConnectionCreated({ id }) {
    logger.info("connection.created", { connectionId: id });
    try {
      await refreshConnectionList();
      if (!connectTo(id)) {
        showToast({ type: "error", title: t("notifications.connectionSaveFailed") });
        return;
      }
      showToast({ type: "success", title: t("notifications.connectionSaved") });
      navigate("workspace");
    } catch (error) {
      showToast({
        type: "error",
        title: t("notifications.connectionSaveFailed"),
        message: String(error),
      });
    }
  }

  async function runRightSidebarTransition(updateLayout) {
    await runViewTransition(
      async () => {
        updateLayout();
        await nextTick();
      },
      { className: WORKSPACE_SIDEBAR_TRANSITION_CLASS },
    );
  }

  function applyWorkspaceSplitLayout(layout) {
    if (!isShellTab.value) return;
    const sidebarWidth = Number(layout?.[1]);
    if (!Number.isFinite(sidebarWidth)) return;
    const nextWidth = Math.min(
      RIGHT_SIDEBAR_MAX_WIDTH,
      Math.max(RIGHT_SIDEBAR_MIN_WIDTH, Math.round(sidebarWidth)),
    );
    if (rightSidebarWidth.value !== nextWidth) {
      rightSidebarWidth.value = nextWidth;
    }
  }

  const handleWorkspaceSplitLayout = createRafThrottle(applyWorkspaceSplitLayout);

  function toggleRightSidebarView(view) {
    if (!isShellTab.value) return;
    const nextView = rightSidebarOpen.value && rightSidebarView.value === view ? null : view;
    void runRightSidebarTransition(() => {
      if (nextView && !SERVICE_SIDEBAR_VIEWS.has(nextView) && sessionTabs.value.length === 0) {
        rightSidebarView.value = null;
        return;
      }
      rightSidebarView.value = nextView;
    }).catch(() => {});
  }

  function openTerminalSearch() {
    if (!activeConnectionInfo.value || activeTab.value !== "shell") return;
    workspaceUi.requestTerminalSearch();
  }

  function redetectActiveSerialBaud() {
    const connectionId = activeConnectionInfo.value?.connectionId || activeConnection.value;
    const started = reconnectSerialAutoBaud(connectionId);
    if (!started) {
      showToast({ type: "error", title: t("notifications.serialBaudUnavailable") });
      return;
    }

    const toastId = showToast({
      type: "loading",
      title: t("notifications.serialBaudDetecting"),
      message: t("notifications.serialBaudDetectingDesc"),
    });
    pendingSerialRedetect.value = { connectionId, toastId };
  }

  async function toggleActiveSessionRecording() {
    if (!isShellTab.value) return;
    const connectionId = activeConnection.value;
    if (!connectionId) return;

    const wasRecording = !!sessionRecordings.value.get(connectionId)?.active;
    logger.info("recording.toggle", { connectionId, active: wasRecording });
    try {
      const changed = await toggleSessionRecording(connectionId);
      if (!changed) return;
      const state = sessionRecordings.value.get(connectionId);
      showToast({
        type: "success",
        title: wasRecording
          ? t("notifications.sessionRecordingStopped")
          : t("notifications.sessionRecordingStarted"),
        message: wasRecording ? "" : state?.path || "",
      });
    } catch (error) {
      showToast({
        type: "error",
        title: t("notifications.sessionRecordingFailed"),
        message: String(error),
      });
    }
  }

  function handleWorkspaceTabbarAction(actionId) {
    if (!isShellTab.value) return;

    if (actionId === "panel-session") {
      toggleRightSidebarView("session");
      return;
    }
    if (actionId === "panel-performance") {
      toggleRightSidebarView("performance");
      return;
    }
    if (actionId === "panel-proxy") {
      toggleRightSidebarView("proxy");
      return;
    }
    if (actionId === "panel-file-service") {
      toggleRightSidebarView("file-service");
      return;
    }
    if (actionId === "quick-search") {
      openTerminalSearch();
      return;
    }
    if (actionId === "quick-run-script") {
      openScriptRunPicker();
      return;
    }
    if (actionId === "quick-recording") {
      toggleActiveSessionRecording();
      return;
    }
    if (actionId === "quick-redetect-baud") {
      redetectActiveSerialBaud();
    }
  }

  onBeforeUnmount(() => {
    handleWorkspaceSplitLayout.cancel();
  });

  return {
    handleWorkspaceSplitLayout,
    handleWorkspaceTabbarAction,
    navItems,
    onConnectionCreated,
    onConnectTo,
    rightSidebarMaxWidth: RIGHT_SIDEBAR_MAX_WIDTH,
    rightSidebarMinWidth: RIGHT_SIDEBAR_MIN_WIDTH,
    rightSidebarOpen,
    rightSidebarView,
    rightSidebarWidth,
    rightSidebarSearchToken: terminalSearchOpenToken,
    tabbarSideButtons,
    toggleActiveSessionRecording,
  };
}
