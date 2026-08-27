<script setup>
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { storeToRefs } from "pinia";
import { Cable, Globe2, Network, Pencil, Plus, Server, Trash2 } from "@lucide/vue";
import "../styles/sessions.scss";
import { useWorkspaceStore } from "../stores/workspaceStore";
import AddConnectionDialog from "../components/AddConnectionDialog.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import { useRoute, useRouter } from "vue-router";
import { useDialogExitTeardown } from "../composables/useDialogExitTeardown";
import { useSortableList } from "../composables/useSortableList";
import { useToasts } from "../composables/useToasts";
import { createLogger } from "../utils/logger";
import { noop } from "../utils/noop";
import {
  connectionEndpointLabel,
  isSerialProtocol,
  isTelnetProtocol,
  protocolDisplayClass,
} from "../utils/connectionProtocols";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const workspace = useWorkspaceStore();
const { connectionProfiles } = storeToRefs(workspace);
const { connectTo, refreshConnectionList, removeConnection, reorderConnections } = workspace;
const { showToast } = useToasts();
const logger = createLogger("frontend.sessions");

const editingConn = ref(null);
const editorOpen = ref(false);
const pendingDelete = ref(null);
const listRef = ref(null);
const { scheduleExitTeardown, cancelExitTeardown } = useDialogExitTeardown();

const CLICK_SUPPRESS_MS = 180;
let suppressClickTimer = 0;
let suppressNextClick = false;

function connectionIds() {
  return connectionProfiles.value.map((connection) => connection.id);
}

function suppressUpcomingClick() {
  suppressNextClick = true;
  if (suppressClickTimer) window.clearTimeout(suppressClickTimer);
  suppressClickTimer = window.setTimeout(() => {
    suppressNextClick = false;
    suppressClickTimer = 0;
  }, CLICK_SUPPRESS_MS);
}

const { dragging, sortableCleanup, createSortable, destroySortable } = useSortableList({
  listRef,
  ids: connectionIds,
  draggable: ".session-card",
  filter: ".session-card-actions, .session-card-actions *",
  delay: 0,
  delayOnTouchOnly: true,
  onDragEnd: suppressUpcomingClick,
  onReorder: reorderConnections,
  onReorderError(error) {
    showToast({
      type: "error",
      title: t("notifications.connectionOrderSaveFailed"),
      message: String(error),
    });
  },
});

async function onSave({ id }) {
  logger.info("connection.save", id, editingConn.value ? "(edit)" : "(new)");
  try {
    if (editingConn.value) {
      await refreshConnectionList();
      showToast({ type: "success", title: t("notifications.connectionSaved") });
    } else {
      refreshConnectionList().catch(noop);
      if (connectTo(id)) {
        showToast({ type: "success", title: t("notifications.connectionSaved") });
        router.push({ name: "workspace" });
      }
    }
  } catch (error) {
    showToast({
      type: "error",
      title: t("notifications.connectionSaveFailed"),
      message: String(error),
    });
  }
  // 保存成功后弹窗会自行关闭（update:open），编辑实例由 onEditorOpenChange
  // 在退出动画结束后统一销毁，这里不再立即置空。
}

function onConnect(conn) {
  if (dragging.value || suppressNextClick) {
    suppressNextClick = false;
    if (suppressClickTimer) {
      window.clearTimeout(suppressClickTimer);
      suppressClickTimer = 0;
    }
    return;
  }
  logger.info("connection.connect", conn.name);
  connectTo(conn.id);
  router.push({ name: "workspace" });
}

function requestRemove(conn) {
  pendingDelete.value = conn;
}

function onPendingDeleteOpenChange(value) {
  if (!value) pendingDelete.value = null;
}

function clearEditQuery() {
  if (!route.query.edit) return;
  const query = { ...route.query };
  delete query.edit;
  router.replace({ name: "sessions", query }).catch(noop);
}

function openConnectionEditorFromRoute() {
  if (route.name !== "sessions") return;
  const editId = String(route.query.edit || "");
  if (!editId) return;
  const connection = connectionProfiles.value.find((item) => item.id === editId);
  if (!connection) return;
  void openConnectionEditor(connection);
  clearEditQuery();
}

async function openConnectionEditor(connection) {
  if (!connection) return;
  cancelExitTeardown();
  editorOpen.value = false;
  editingConn.value = null;
  await nextTick();
  editingConn.value = connection;
  await nextTick();
  editorOpen.value = true;
}

function onEditorOpenChange(value) {
  editorOpen.value = value;
  // 编辑实例由 v-if 挂载，立即置空会跳过退出动画直接卸载；延迟到动画
  // 结束后再销毁，内容与高度在退出期间保持稳定。
  if (!value) {
    scheduleExitTeardown(() => {
      editingConn.value = null;
    });
  }
}

async function confirmRemove() {
  if (!pendingDelete.value) return;
  if (pendingDelete.value.external) {
    pendingDelete.value = null;
    return;
  }
  await removeConnection(pendingDelete.value.id);
  pendingDelete.value = null;
  showToast({ type: "success", title: t("notifications.connectionDeleted") });
}

watch(
  [() => route.name, () => route.query.edit, () => connectionProfiles.value],
  openConnectionEditorFromRoute,
  { immediate: true },
);

onMounted(() => {
  nextTick(createSortable);
  sortableCleanup.bindReleaseCleanup();
});

onBeforeUnmount(() => {
  destroySortable();
  if (suppressClickTimer) window.clearTimeout(suppressClickTimer);
  sortableCleanup.unbindReleaseCleanup();
});
</script>

<template>
  <div class="sessions-root">
    <div class="ui-page-header">
      <div class="ui-page-header-main">
        <Network
          :size="18"
          stroke-width="1.6"
          class="text-accent"
        />
        <div>
          <h2 class="ui-page-title">
            {{ t("settings.sections.sessions") }}
          </h2>
          <p class="ui-page-desc">
            {{ t("settings.sessions.description") }}
          </p>
        </div>
      </div>
      <AddConnectionDialog
        :edit-connection="null"
        :connections="connectionProfiles"
        @save="onSave"
      >
        <button
          type="button"
          class="ui-button-primary flex items-center gap-[6px] text-[0.8571em]"
        >
          <Plus
            :size="13"
            stroke-width="2"
          />
          {{ t("actions.addConnection") }}
        </button>
      </AddConnectionDialog>
      <AddConnectionDialog
        v-if="editingConn"
        :key="editingConn.id"
        :open="editorOpen"
        :edit-connection="editingConn"
        :connections="connectionProfiles"
        @update:open="onEditorOpenChange"
        @save="onSave"
      />
    </div>

    <div
      ref="listRef"
      class="sessions-list"
      :class="{ 'sessions-list-dragging': dragging }"
    >
      <div
        v-if="!connectionProfiles.length"
        class="ui-empty-state col-[1/-1] px-[24px] py-[60px] text-[0.9286em]"
      >
        <Network
          :size="32"
          stroke-width="1.2"
          class="text-text-tertiary mb-[12px]"
        />
        <p>{{ t("settings.sessions.empty") }}</p>
      </div>

      <div
        v-for="conn in connectionProfiles"
        :key="conn.id"
        class="session-card"
        :data-id="conn.id"
        @click="onConnect(conn)"
      >
        <div
          class="session-card-status"
          :class="protocolDisplayClass(conn.protocol)"
        >
          <Cable
            v-if="isSerialProtocol(conn.protocol)"
            class="session-card-protocol-icon"
            :size="18"
            stroke-width="1.8"
            aria-hidden="true"
          />
          <Globe2
            v-else-if="isTelnetProtocol(conn.protocol)"
            class="session-card-protocol-icon"
            :size="18"
            stroke-width="1.8"
            aria-hidden="true"
          />
          <Server
            v-else
            class="session-card-protocol-icon"
            :size="18"
            stroke-width="1.8"
            aria-hidden="true"
          />
        </div>
        <div class="session-card-body">
          <div class="session-card-name">
            {{ conn.name }}
          </div>
          <div class="session-card-meta">
            {{ (conn.protocol || "ssh").toUpperCase() }}
            {{ connectionEndpointLabel(conn) }}
          </div>
        </div>
        <div
          class="session-card-actions"
          @click.stop
        >
          <button
            type="button"
            class="ui-row-action"
            :aria-label="t('actions.edit')"
            @click="void openConnectionEditor(conn)"
          >
            <Pencil
              :size="13"
              stroke-width="1.8"
            />
          </button>
          <button
            type="button"
            class="ui-row-action ui-row-action-danger"
            :aria-label="t('actions.delete')"
            @click="requestRemove(conn)"
          >
            <Trash2
              :size="13"
              stroke-width="1.8"
            />
          </button>
        </div>
      </div>
    </div>

    <ConfirmDialog
      :open="Boolean(pendingDelete)"
      tone="danger"
      :title="t('settings.sessions.deleteConfirm.title')"
      :description="
        t('settings.sessions.deleteConfirm.description', { name: pendingDelete?.name || '' })
      "
      :confirm-text="t('settings.sessions.deleteConfirm.confirm')"
      :confirm-icon="Trash2"
      @update:open="onPendingDeleteOpenChange"
      @confirm="confirmRemove"
    />
  </div>
</template>
