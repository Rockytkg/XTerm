<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { FolderTree } from "@lucide/vue";
import { useToasts } from "../../composables/useToasts";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { setBackendEncodingDetection } from "../../services/terminalSessions";
import { connectionCan } from "../../utils/connectionCapabilities";
import UiSwitch from "../UiSwitch.vue";
import UiSelect from "../UiSelect.vue";
import {
  BACKSPACE_SENDS,
  BACKSPACE_SENDS_OPTIONS,
  DEFAULT_TERMINAL_ENCODING,
  DEFAULT_TERMINAL_TYPE,
  TERMINAL_TYPE_OPTIONS,
  createEncodingOptions,
} from "../../utils/terminalSessionOptions";

const props = defineProps({
  activeConnection: { type: Object, default: null },
  activeConnectionState: { type: Object, default: () => ({ status: "idle", error: null }) },
  workingDirectory: { type: String, required: true },
});

const { t } = useI18n();
const { updateConnection, sessionRegistry } = useWorkspaceStore();
const { showToast } = useToasts();

const protocol = computed(() => props.activeConnection?.protocol);
const protocolLabel = computed(() => protocol.value?.toUpperCase() || "—");
const isSerial = computed(() => connectionCan(props.activeConnection, "serialBaudDetection"));
const isRemoteShell = computed(
  () =>
    connectionCan(props.activeConnection, "metrics") ||
    connectionCan(props.activeConnection, "sftp"),
);

const serialPort = computed(
  () => props.activeConnectionState?.detectedSerialPort || props.activeConnection?.port || "—",
);
const baudRate = computed(
  () => props.activeConnectionState?.detectedBaudRate || props.activeConnection?.baudRate || "—",
);

const terminalType = computed(() => props.activeConnection?.terminalType || DEFAULT_TERMINAL_TYPE);
const encoding = computed(() => props.activeConnection?.encoding || DEFAULT_TERMINAL_ENCODING);
const backspaceSends = computed(
  () => props.activeConnection?.backspaceSends || BACKSPACE_SENDS.DEL,
);

const encodingOptions = computed(() => createEncodingOptions(t));
const profileConnectionId = computed(
  () => props.activeConnection?.connectionId || props.activeConnection?.id || "",
);
const frontendSessionId = computed(() => props.activeConnection?.id || "");
const backendSessionId = computed(() => props.activeConnection?.sessionId || "");

async function persistProfileField(field, value) {
  if (!profileConnectionId.value) return;
  try {
    await updateConnection(profileConnectionId.value, { [field]: value });
  } catch (error) {
    showToast({
      type: "error",
      title: t("notifications.connectionSaveFailed"),
      message: error?.message || String(error),
    });
  }
}

function handleEncodingChange(value) {
  const normalized = value === DEFAULT_TERMINAL_ENCODING ? undefined : value;
  void persistProfileField("encoding", normalized);
  if (backendSessionId.value) {
    const channelId = sessionRegistry.getActiveSessionChannelId(frontendSessionId.value);
    setBackendEncodingDetection({
      sessionId: backendSessionId.value,
      channelId,
      enabled: !normalized,
      encoding: normalized || null,
    }).catch(() => {});
  }
}
</script>

<template>
  <div class="workspace-sidebar-pane workspace-sidebar-pane-session">
    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-section-head">
        <div class="workspace-sidebar-section-icon">
          <FolderTree
            :size="15"
            stroke-width="1.8"
          />
        </div>
        <div class="min-w-0">
          <div class="workspace-sidebar-section-kicker">
            {{ t("overview.section") }}
          </div>
          <div class="workspace-sidebar-section-title">
            {{ activeConnection.name }}
          </div>
        </div>
        <span
          class="workspace-sidebar-status-pill"
          :class="
            activeConnection.status === 'online'
              ? 'workspace-sidebar-status-online'
              : activeConnection.status === 'warning'
                ? 'workspace-sidebar-status-warning'
                : ''
          "
        >
          {{ t(`status.${activeConnection.status || "offline"}`) }}
        </span>
      </div>

      <div class="workspace-sidebar-endpoint">
        {{ protocolLabel }} · {{ activeConnection.host || activeConnection.port || "-" }}
      </div>

      <div class="workspace-sidebar-hero-grid">
        <!-- SSH -->
        <template v-if="isRemoteShell">
          <div class="workspace-sidebar-hero-metric">
            <span>{{ t("overview.session.user") }}</span>
            <strong class="truncate">{{ activeConnection.user || "—" }}</strong>
          </div>
          <div class="workspace-sidebar-hero-metric">
            <span>{{ t("overview.session.path") }}</span>
            <strong class="truncate">{{ workingDirectory || "—" }}</strong>
          </div>
        </template>

        <!-- Serial -->
        <template v-else-if="isSerial">
          <div class="workspace-sidebar-hero-metric">
            <span>{{ t("overview.session.serialPort") }}</span>
            <strong class="truncate">{{ serialPort }}</strong>
          </div>
          <div class="workspace-sidebar-hero-metric">
            <span>{{ t("overview.session.baudRate") }}</span>
            <strong>{{ baudRate }}</strong>
          </div>
        </template>

        <!-- Telnet -->
        <template v-else>
          <div class="workspace-sidebar-hero-metric">
            <span>{{ t("overview.session.terminalType") }}</span>
            <strong>{{ terminalType }}</strong>
          </div>
          <div
            v-if="activeConnection.user"
            class="workspace-sidebar-hero-metric"
          >
            <span>{{ t("overview.session.user") }}</span>
            <strong class="truncate">{{ activeConnection.user }}</strong>
          </div>
        </template>
      </div>
    </section>

    <section class="workspace-sidebar-section">
      <div class="workspace-sidebar-section-kicker">
        {{ t("overview.session.title") }}
      </div>
      <div class="workspace-sidebar-pref-list">
        <div class="workspace-sidebar-pref-row">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("overview.session.toggleHighlight")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("connectionDialog.fields.highlightEnabledHint")
            }}</span>
          </div>
          <UiSwitch
            :model-value="activeConnection?.terminalHighlightEnabled !== false"
            @update:model-value="persistProfileField('terminalHighlightEnabled', $event)"
          />
        </div>

        <div class="workspace-sidebar-pref-row">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("connectionDialog.fields.morePromptCleanup")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("connectionDialog.fields.morePromptCleanupHint")
            }}</span>
          </div>
          <UiSwitch
            :model-value="activeConnection?.terminalMorePromptCleanup === true"
            @update:model-value="persistProfileField('terminalMorePromptCleanup', $event)"
          />
        </div>

        <div class="workspace-sidebar-pref-row workspace-sidebar-pref-row-stack">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("connectionDialog.fields.terminalType")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("connectionDialog.fields.terminalTypeHint")
            }}</span>
          </div>
          <UiSelect
            :model-value="terminalType"
            :options="TERMINAL_TYPE_OPTIONS"
            @update:model-value="persistProfileField('terminalType', $event)"
          />
        </div>

        <div class="workspace-sidebar-pref-row workspace-sidebar-pref-row-stack">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("connectionDialog.fields.encoding")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("connectionDialog.fields.encodingHint")
            }}</span>
          </div>
          <UiSelect
            :model-value="encoding"
            :options="encodingOptions"
            @update:model-value="handleEncodingChange($event)"
          />
        </div>

        <div class="workspace-sidebar-pref-row workspace-sidebar-pref-row-stack">
          <div class="workspace-sidebar-pref-text">
            <span class="workspace-sidebar-pref-label">{{
              t("connectionDialog.fields.backspaceSends")
            }}</span>
            <span class="workspace-sidebar-pref-hint">{{
              t("connectionDialog.fields.backspaceSendsHint")
            }}</span>
          </div>
          <UiSelect
            :model-value="backspaceSends"
            :options="BACKSPACE_SENDS_OPTIONS"
            @update:model-value="persistProfileField('backspaceSends', $event)"
          />
        </div>
      </div>
    </section>
  </div>
</template>
