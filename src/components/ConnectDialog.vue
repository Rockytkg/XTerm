<script setup>
import { computed, ref, useSlots } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  DialogTrigger,
} from "reka-ui";
import { ArrowRight, Cable, Globe2, ListChecks, Plus, Server, X } from "@lucide/vue";
import AddConnectionDialog from "./AddConnectionDialog.vue";
import { createLogger } from "../utils/logger";
import {
  connectionEndpointLabel,
  isSerialProtocol,
  isTelnetProtocol,
  protocolDisplayClass,
} from "../utils/connectionProtocols";
import "../styles/dialogs-connect.scss";

const props = defineProps({
  connections: { type: Array, required: true },
  allowCreateConnection: { type: Boolean, default: true },
  open: { type: Boolean, default: undefined },
  protocolFilter: { type: String, default: "" },
});
const emit = defineEmits(["connect", "connection-created", "update:open"]);
const { t } = useI18n();
const slots = useSlots();
const logger = createLogger("frontend.connection.connect_dialog");

const internalOpen = ref(false);
const createDialogOpen = ref(false);
const dialogOpen = computed({
  get: () => props.open ?? internalOpen.value,
  set(value) {
    internalOpen.value = value;
    emit("update:open", value);
  },
});
const selectableConnections = computed(() => {
  const protocolFilter = props.protocolFilter.trim().toLowerCase();
  if (!protocolFilter) return props.connections;
  return props.connections.filter((conn) => (conn.protocol || "ssh") === protocolFilter);
});
const emptyMessage = computed(() => {
  if (props.protocolFilter === "ssh" && !props.allowCreateConnection) {
    return t("connectDialog.emptySshOnly");
  }
  return t("connectDialog.empty");
});

function protocolClass(protocol) {
  return protocolDisplayClass(protocol);
}

function protocolIcon(protocol) {
  if (isSerialProtocol(protocol)) return Cable;
  if (isTelnetProtocol(protocol)) return Globe2;
  return Server;
}

function connectionEndpoint(conn) {
  return connectionEndpointLabel(conn);
}

function connect(conn) {
  logger.info("connection.connect", { name: conn.name, protocol: conn.protocol });
  emit("connect", conn.id);
  dialogOpen.value = false;
}

function openCreateDialog() {
  if (!props.allowCreateConnection) return;
  createDialogOpen.value = true;
}

function onCreateDialogOpenChange(value) {
  createDialogOpen.value = value;
  if (!value && dialogOpen.value) {
    dialogOpen.value = false;
  }
}

function onConnectionCreated(payload) {
  createDialogOpen.value = false;
  dialogOpen.value = false;
  emit("connection-created", payload);
}
</script>

<template>
  <DialogRoot v-model:open="dialogOpen">
    <DialogTrigger
      v-if="slots.default"
      as-child
    >
      <slot />
    </DialogTrigger>
    <DialogPortal>
      <DialogOverlay class="dialog-overlay connect-dialog-overlay" />
      <DialogContent class="dialog-content connect-dialog-content focus:outline-none">
        <header class="connect-dialog-header">
          <div
            class="connect-dialog-header-icon"
            aria-hidden="true"
          >
            <ListChecks
              :size="18"
              stroke-width="1.8"
            />
          </div>
          <div class="connect-dialog-heading">
            <DialogTitle class="connect-dialog-title">
              {{ t("connectDialog.title") }}
            </DialogTitle>
            <DialogDescription class="connect-dialog-desc">
              {{ t("connectDialog.description") }}
            </DialogDescription>
          </div>
          <DialogClose as-child>
            <button
              type="button"
              class="ui-icon-button connect-dialog-close"
              :aria-label="t('actions.closeDialog')"
            >
              <X
                :size="16"
                stroke-width="1.8"
              />
            </button>
          </DialogClose>
        </header>

        <div class="connect-dialog-body">
          <div
            v-if="!selectableConnections.length"
            class="connect-dialog-empty"
          >
            <div
              class="connect-dialog-empty-icon session-card-status session-card-status-ssh"
              aria-hidden="true"
            >
              <Server
                class="session-card-protocol-icon"
                :size="20"
                stroke-width="1.7"
              />
            </div>
            <p>{{ emptyMessage }}</p>
          </div>
          <button
            v-for="conn in selectableConnections"
            :key="conn.id"
            type="button"
            class="connect-dialog-option"
            :aria-label="`${t('actions.connect')} ${conn.name || connectionEndpoint(conn)}`"
            @click="connect(conn)"
          >
            <span
              class="connect-dialog-option-status session-card-status"
              :class="protocolClass(conn.protocol)"
              aria-hidden="true"
            >
              <component
                :is="protocolIcon(conn.protocol)"
                class="session-card-protocol-icon"
                :size="18"
                stroke-width="1.8"
              />
            </span>
            <span class="connect-dialog-option-main">
              <span class="connect-dialog-option-name">{{ conn.name || "-" }}</span>
              <span class="connect-dialog-option-endpoint">{{ connectionEndpoint(conn) }}</span>
            </span>
            <span class="connect-dialog-option-side">
              <span class="connect-dialog-protocol">{{
                (conn.protocol || "ssh").toUpperCase()
              }}</span>
              <span class="connect-dialog-action">
                <span>{{ t("actions.connect") }}</span>
                <ArrowRight
                  :size="13"
                  stroke-width="1.9"
                  aria-hidden="true"
                />
              </span>
            </span>
          </button>
        </div>

        <footer class="connect-dialog-footer">
          <DialogClose as-child>
            <button
              type="button"
              class="ui-button-secondary"
            >
              {{ t("actions.cancel") }}
            </button>
          </DialogClose>
          <button
            v-if="allowCreateConnection"
            type="button"
            class="ui-button-primary connect-dialog-new"
            @click="openCreateDialog"
          >
            <Plus
              :size="13"
              stroke-width="2"
            />
            {{ t("actions.newConnection") }}
          </button>
        </footer>
        <AddConnectionDialog
          :open="createDialogOpen"
          :connections="connections"
          @update:open="onCreateDialogOpenChange"
          @save="onConnectionCreated"
        />
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
