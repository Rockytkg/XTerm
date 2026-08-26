<script setup>
import { ref, computed, nextTick, onBeforeUnmount, onMounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  ToggleGroupItem,
  ToggleGroupRoot,
} from "reka-ui";
import {
  CirclePlus,
  FileKey,
  GitBranch,
  Grid2X2,
  KeyRound,
  Lock,
  Pencil,
  ShieldCheck,
  SquarePen,
  Trash2,
  X,
} from "@lucide/vue";
import Sortable from "sortablejs";
import "../styles/connection-dialog.scss";
// Per-view import: this component renders independently from other consumers of this stylesheet.
import "../styles/settings-tab-switcher.scss";
import "../styles/views-relationship.scss";
import AppTooltip from "../components/AppTooltip.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import CredentialGraphView from "./CredentialGraphView.vue";
import { appFieldNames, NO_NATIVE_AUTOCOMPLETE } from "../utils/autocomplete";
import {
  deleteUnusedCredentials,
  loadCredentialUsages,
  loadCredentials,
  reorderCredentials,
} from "../services/credentials";
import { sortableMotion } from "../utils/motion";
import { loadWorkspaceBootstrap } from "../services/workspace";
import { useAppPreferences } from "../composables/useAppPreferences";
import { useCredentialEditor } from "../composables/useCredentialEditor";
import { useDialogExitTeardown } from "../composables/useDialogExitTeardown";
import {
  normalizeCredentialUsages,
  useCredentialDeleteFlow,
} from "../composables/useCredentialDeleteFlow";
import { useToasts } from "../composables/useToasts";
import { supportsSavedCredential } from "../utils/connectionProtocols";
import {
  buildCredentialTypeChangeImpact,
  CREDENTIAL_TYPE_CHANGE_ACTION,
} from "../utils/credentialTypeChange";
import { createSortableCleanup } from "../utils/sortableCleanup";
import { createLogger } from "../utils/logger";
import { noop } from "../utils/noop";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const { showToast } = useToasts();
const { preferences } = useAppPreferences();
const logger = createLogger("frontend.keys");

const graphView = ref(null);
const credentials = ref([]);
const connections = ref([]);
const cleanupConfirmOpen = ref(false);
const isCleaning = ref(false);
const typeChangeConfirm = ref(null);
const typeChangeDialogOpen = ref(false);
const credentialUsages = ref([]);
const layoutModes = Object.freeze(["graph", "cards"]);
const listRef = ref(null);
const dragging = ref(false);
const { scheduleExitTeardown, cancelExitTeardown } = useDialogExitTeardown();

const CREDENTIAL_ORDER_SEPARATOR = "\u001f";
const SORTABLE_STATE_CLASSES = [
  "session-card-sortable-chosen",
  "session-card-sortable-ghost",
  "session-card-sortable-drag",
  "session-card-sortable-fallback",
];

let sortable = null;
let unmounted = false;
const sortableCleanup = createSortableCleanup({
  classNames: SORTABLE_STATE_CLASSES,
  onReset: () => {
    dragging.value = false;
  },
});

const {
  addingType,
  buildCredentialSaveRequest,
  canSave,
  closeEditor,
  credentialDialogOpen,
  credentialDialogTitle,
  editingId,
  formError,
  keyForm,
  onCredentialDialogOpenChange,
  passwordForm,
  persistCredentialRequest,
  pickPrivateKeyFile,
  selectCredentialType,
  startAdd,
  startEdit,
} = useCredentialEditor({
  t,
  showToast,
});

onMounted(async () => {
  await loadCredentialState();
  if (unmounted) return;
  openCredentialEditorFromRoute();
  await nextTick();
  if (unmounted) return;
  createSortable();
  sortableCleanup.bindReleaseCleanup();
});

const credentialLayoutMode = computed({
  get() {
    return layoutModes.includes(preferences.credentialLayoutMode)
      ? preferences.credentialLayoutMode
      : "graph";
  },
  set(value) {
    preferences.credentialLayoutMode = layoutModes.includes(value) ? value : "graph";
  },
});

const isGraphLayout = computed(() => credentialLayoutMode.value === "graph");

async function loadCredentialState() {
  formError.value = "";
  try {
    const [saved, usages, workspace] = await Promise.all([
      loadCredentials(),
      loadCredentialUsages(),
      loadWorkspaceBootstrap(),
    ]);
    credentials.value = Array.isArray(saved) ? saved : [];
    credentialUsages.value = Array.isArray(usages) ? usages : [];
    connections.value = Array.isArray(workspace?.connections) ? workspace.connections : [];
  } catch (error) {
    formError.value = String(error);
  }
}

function clearEditQuery() {
  if (!route.query.edit) return;
  const query = { ...route.query };
  delete query.edit;
  router.replace({ name: "keys", query }).catch(noop);
}

function openCredentialEditorFromRoute() {
  if (route.name !== "keys") return;
  const editId = String(route.query.edit || "");
  if (!editId) return;
  const credential = credentials.value.find((item) => item.id === editId);
  if (!credential) return;
  startEdit(credential);
  clearEditQuery();
}

function onCredentialDeleteOpenChange(value) {
  if (!value) cancelCredentialDelete();
}

async function addCredential() {
  const request = buildCredentialSaveRequest();
  if (!request) return;
  const impact = buildCredentialTypeChangeImpact({
    credential: request.credential,
    nextType: request.payload.credType,
    usages: credentialUsagesFor(request.credential?.id),
    connections: connections.value,
  });
  if (impact.needsConfirmation) {
    cancelExitTeardown();
    typeChangeConfirm.value = { impact, request };
    typeChangeDialogOpen.value = true;
    return;
  }
  await persistCredentialSave(request);
}

async function persistCredentialSave(request) {
  const previousEditingId = request.mode === "updateExisting" ? request.credential?.id : "";
  const saved = await persistCredentialRequest(request);
  if (!saved) return;

  credentials.value = previousEditingId
    ? credentials.value.map((credential) =>
        credential.id === previousEditingId ? saved : credential,
      )
    : [...credentials.value, saved];
  await refreshCredentialUsages();
  await graphView.value?.refreshGraph?.();
  closeEditor();
  showToast({ type: "success", title: t("notifications.credentialSaved") });
}

async function resolveTypeChangeConfirm(action) {
  const pending = typeChangeConfirm.value;
  typeChangeDialogOpen.value = false;
  if (!pending) return;
  // 弹窗立即开始退出动画，impact 数据延迟清理：slot 里的受影响引用
  // 列表若在关闭瞬间清空，弹壳会在消失途中塌掉一节高度。
  scheduleExitTeardown(() => {
    typeChangeConfirm.value = null;
  });
  if (action === CREDENTIAL_TYPE_CHANGE_ACTION.CANCEL) return;
  await persistCredentialSave({
    ...pending.request,
    mode: action,
  });
}

async function refreshCredentialUsages() {
  try {
    const [usages, workspace] = await Promise.all([
      loadCredentialUsages(),
      loadWorkspaceBootstrap(),
    ]);
    credentialUsages.value = Array.isArray(usages) ? usages : [];
    connections.value = Array.isArray(workspace?.connections) ? workspace.connections : [];
  } catch (error) {
    logger.warn("credential-usages.refresh.failed", error);
  }
}

function requestRemove(credential) {
  logger.info("credential.remove.requested", credential?.id);
  formError.value = "";
  requestCredentialDelete(credential);
}

function requestCleanupUnused() {
  cleanupConfirmOpen.value = true;
  formError.value = "";
}

async function confirmCleanupUnused() {
  if (isCleaning.value) return;
  isCleaning.value = true;
  formError.value = "";
  try {
    const result = await deleteUnusedCredentials();
    const deletedIds = Array.isArray(result?.deletedIds) ? result.deletedIds : [];
    if (deletedIds.length) {
      const deleted = new Set(deletedIds);
      credentials.value = credentials.value.filter((cred) => !deleted.has(cred.id));
    }
    await refreshCredentialUsages();
    await graphView.value?.refreshGraph?.();
    cleanupConfirmOpen.value = false;
    showToast({
      type: "success",
      title: t("notifications.credentialCleanupSucceeded"),
      message: t("credentials.cleanup.deletedCount", { count: deletedIds.length }),
    });
  } catch (error) {
    const message = String(error);
    formError.value = message;
    showToast({
      type: "error",
      title: t("notifications.credentialCleanupFailed"),
      message,
    });
  } finally {
    isCleaning.value = false;
  }
}

const credentialUsageMap = computed(() => {
  const usageMap = new Map();
  for (const usage of credentialUsages.value) {
    if (!usage?.credentialId) continue;
    const list = usageMap.get(usage.credentialId) || [];
    list.push(usage);
    usageMap.set(usage.credentialId, list);
  }
  return usageMap;
});
const usedCount = computed(
  () => credentials.value.filter((cred) => credentialUsageMap.value.has(cred.id)).length,
);
const unusedCount = computed(() => Math.max(0, credentials.value.length - usedCount.value));
const graphConnections = computed(() =>
  connections.value.filter((connection) => supportsSavedCredential(connection?.protocol || "ssh")),
);
const relationshipEdgeCount = computed(
  () =>
    graphConnections.value.filter(
      (connection) =>
        supportsSavedCredential(connection?.protocol || "ssh") && connection.savedCredentialId,
    ).length,
);
function credentialUsagesFor(credentialId) {
  return credentialUsageMap.value.get(credentialId) || [];
}

function typeChangeUsageNames(usages) {
  return usages
    .map((usage) => usage.connectionName || usage.connectionId)
    .filter(Boolean)
    .join(", ");
}

function credentialIds() {
  return credentials.value.map((credential) => credential.id);
}

function sameOrder(a, b) {
  return a.length === b.length && a.every((id, index) => id === b[index]);
}

function syncSortableOrder() {
  if (!sortable || dragging.value) return;
  const ids = credentialIds();
  const domOrder = sortable.toArray().filter(Boolean);
  sortable.option("disabled", ids.length < 2);
  if (!sameOrder(domOrder, ids)) {
    sortable.sort(ids, false);
  }
}

function createSortable() {
  const list = listRef.value;
  if (!list || sortable || isGraphLayout.value) return;

  sortable = Sortable.create(list, {
    ...sortableMotion,
    draggable: ".cred-card",
    dataIdAttr: "data-id",
    delay: 170,
    delayOnTouchOnly: false,
    touchStartThreshold: 4,
    fallbackClass: "session-card-sortable-fallback",
    fallbackTolerance: 5,
    forceFallback: true,
    fallbackOnBody: true,
    scroll: true,
    bubbleScroll: false,
    scrollSensitivity: 48,
    scrollSpeed: 14,
    swapThreshold: 0.62,
    ghostClass: "session-card-sortable-ghost",
    chosenClass: "session-card-sortable-chosen",
    dragClass: "session-card-sortable-drag",
    filter: ".cred-card-actions, .cred-card-actions *",
    preventOnFilter: false,
    onStart() {
      dragging.value = true;
    },
    async onEnd() {
      const nextOrder = sortable.toArray().filter(Boolean);
      sortableCleanup.resetSortableState();

      if (!sameOrder(nextOrder, credentialIds())) {
        const previousCredentials = [...credentials.value];
        const credentialById = new Map(
          credentials.value.map((credential) => [credential.id, credential]),
        );
        credentials.value = nextOrder.map((id) => credentialById.get(id)).filter(Boolean);
        try {
          await reorderCredentials(nextOrder);
          await loadCredentialState();
        } catch (error) {
          credentials.value = previousCredentials;
          showToast({
            type: "error",
            title: t("notifications.credentialOrderSaveFailed"),
            message: String(error),
          });
        }
      }

      nextTick(syncSortableOrder);
    },
    onUnchoose() {
      sortableCleanup.resetSortableState();
    },
  });

  syncSortableOrder();
}

function destroySortable() {
  sortable?.destroy();
  sortable = null;
  dragging.value = false;
}

const {
  credentialDeleteOpen,
  pendingCredentialDelete,
  credentialDeleteBusy,
  pendingCredentialDeleteDescription,
  requestCredentialDelete,
  confirmCredentialDelete,
  cancelCredentialDelete,
} = useCredentialDeleteFlow({
  t,
  showToast,
  getUsages: (credentialId) => normalizeCredentialUsages(credentialUsages.value, credentialId),
  async onDeleted(pending) {
    credentials.value = credentials.value.filter((credential) => credential.id !== pending.id);
    await refreshCredentialUsages();
    await graphView.value?.refreshGraph?.();
    if (editingId.value === pending.id) closeEditor();
  },
  onFailed(error) {
    formError.value = String(error);
  },
});

watch(
  [() => route.name, () => route.query.edit, () => credentials.value],
  openCredentialEditorFromRoute,
);

watch(
  () => credentialIds().join(CREDENTIAL_ORDER_SEPARATOR),
  () => {
    nextTick(syncSortableOrder);
  },
);

watch(isGraphLayout, async (graphLayout) => {
  if (graphLayout) {
    destroySortable();
    return;
  }
  await nextTick();
  createSortable();
});

onBeforeUnmount(() => {
  unmounted = true;
  destroySortable();
  sortableCleanup.unbindReleaseCleanup();
});
</script>

<template>
  <div class="cred-root">
    <div class="relationship-toolbar credential-management-toolbar">
      <div class="relationship-title-group">
        <ShieldCheck
          :size="18"
          stroke-width="1.6"
          class="text-accent"
        />
        <div>
          <h2 class="ui-page-title">
            {{ t("credentials.title") }}
          </h2>
          <p class="ui-page-desc">
            {{ t("credentials.description") }}
          </p>
        </div>
      </div>

      <div class="relationship-stats">
        <span>{{ t("relationshipGraph.views.credential") }}</span>
        <span>{{
          t("relationshipGraph.stats.connections", { count: graphConnections.length })
        }}</span>
        <span>{{ t("relationshipGraph.stats.credentials", { count: credentials.length }) }}</span>
        <span>{{ t("relationshipGraph.stats.edges", { count: relationshipEdgeCount }) }}</span>
      </div>

      <div class="relationship-actions credential-management-actions">
        <ToggleGroupRoot
          v-model="credentialLayoutMode"
          type="single"
          class="settings-tab-switcher credential-layout-switch"
          :aria-label="t('credentials.layout.label')"
        >
          <AppTooltip
            :content="t('credentials.layout.graph')"
            side="bottom"
          >
            <ToggleGroupItem
              value="graph"
              class="settings-tab-option credential-layout-option"
              :aria-label="t('credentials.layout.graph')"
            >
              <GitBranch
                class="settings-tab-icon"
                :size="18"
                stroke-width="1.8"
              />
            </ToggleGroupItem>
          </AppTooltip>
          <AppTooltip
            :content="t('credentials.layout.cards')"
            side="bottom"
          >
            <ToggleGroupItem
              value="cards"
              class="settings-tab-option credential-layout-option"
              :aria-label="t('credentials.layout.cards')"
            >
              <Grid2X2
                class="settings-tab-icon"
                :size="18"
                stroke-width="1.8"
              />
            </ToggleGroupItem>
          </AppTooltip>
        </ToggleGroupRoot>

        <button
          type="button"
          class="credential-toolbar-button credential-toolbar-primary"
          @click="startAdd('password')"
        >
          <CirclePlus
            :size="14"
            stroke-width="2"
          />
          <span>{{ t("credentials.add") }}</span>
        </button>
        <button
          type="button"
          class="credential-toolbar-button credential-toolbar-danger"
          :disabled="!unusedCount || isCleaning"
          @click="requestCleanupUnused"
        >
          <Trash2
            :size="14"
            stroke-width="1.8"
          />
          <span>{{ t("credentials.cleanup.action") }}</span>
        </button>
      </div>
    </div>

    <div class="cred-content">
      <CredentialGraphView
        v-if="isGraphLayout"
        ref="graphView"
        embedded
        @state-changed="refreshCredentialUsages"
      />

      <section
        v-else
        class="cred-list-pane"
      >
        <div
          v-if="formError && !addingType"
          class="cred-list-error"
        >
          {{ formError }}
        </div>

        <div
          v-if="!credentials.length && !addingType"
          class="ui-empty-state px-[24px] py-[60px] text-[0.9286em]"
        >
          <ShieldCheck
            :size="32"
            stroke-width="1.2"
            class="text-text-tertiary mb-[12px]"
          />
          <p>{{ t("credentials.empty") }}</p>
        </div>

        <div
          v-else
          ref="listRef"
          class="cred-list"
          :class="{ 'cred-list-dragging': dragging }"
        >
          <div
            v-for="cred in credentials"
            :key="cred.id"
            class="cred-card"
            :data-id="cred.id"
          >
            <div class="cred-card-header">
              <div class="cred-card-main">
                <div
                  class="cred-card-icon"
                  :class="cred.credType === 'key' ? 'cred-icon-key' : 'cred-icon-pw'"
                >
                  <KeyRound
                    v-if="cred.credType === 'key'"
                    :size="15"
                    stroke-width="1.8"
                  />
                  <Lock
                    v-else
                    :size="15"
                    stroke-width="1.8"
                  />
                </div>
                <div class="cred-card-heading">
                  <span
                    class="cred-card-name"
                    :title="cred.name"
                  >{{ cred.name }}</span>
                  <div class="cred-card-badges">
                    <span
                      class="cred-card-badge"
                      :class="cred.credType === 'key' ? 'badge-key' : 'badge-pw'"
                    >
                      {{ t(`credentials.credTypes.${cred.credType}`) }}
                    </span>
                    <span
                      class="cred-card-badge"
                      :class="credentialUsagesFor(cred.id).length ? 'badge-used' : 'badge-unused'"
                    >
                      {{
                        credentialUsagesFor(cred.id).length
                          ? t("credentials.card.usedCount", {
                            count: credentialUsagesFor(cred.id).length,
                          })
                          : t("credentials.card.unused")
                      }}
                    </span>
                  </div>
                  <div class="cred-card-compact-meta">
                    <span class="cred-card-meta-label">{{ t("credentials.card.references") }}</span>
                    <div
                      v-if="credentialUsagesFor(cred.id).length"
                      class="cred-card-usage-list"
                    >
                      <span
                        v-for="usage in credentialUsagesFor(cred.id)"
                        :key="`${usage.connectionId}:${usage.relation}`"
                        class="cred-card-usage-item"
                        :title="usage.connectionName || usage.connectionId"
                      >
                        <span class="cred-card-usage-name">
                          {{ usage.connectionName || usage.connectionId }}
                        </span>
                      </span>
                    </div>
                    <span
                      v-else
                      class="cred-card-empty-value"
                    >
                      {{ t("credentials.card.noReferences") }}
                    </span>
                  </div>
                  <div
                    v-if="cred.comment"
                    class="cred-card-comment-line"
                    :title="cred.comment"
                  >
                    {{ cred.comment }}
                  </div>
                </div>
              </div>
              <div class="cred-card-actions">
                <button
                  type="button"
                  class="ui-row-action"
                  :title="t('actions.edit')"
                  @click="startEdit(cred)"
                >
                  <Pencil
                    :size="13"
                    stroke-width="1.8"
                  />
                </button>
                <button
                  type="button"
                  class="ui-row-action ui-row-action-danger"
                  :title="t('actions.delete')"
                  @click="requestRemove(cred)"
                >
                  <Trash2
                    :size="13"
                    stroke-width="1.8"
                  />
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>

    <DialogRoot
      :open="credentialDialogOpen"
      @update:open="onCredentialDialogOpenChange"
    >
      <DialogPortal>
        <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
        <DialogContent class="dialog-content conn-dialog focus:outline-none">
          <header class="conn-dialog-header">
            <div
              class="conn-dialog-header-icon"
              aria-hidden="true"
            >
              <component
                :is="editingId ? SquarePen : CirclePlus"
                :size="16"
                stroke-width="1.8"
              />
            </div>
            <div class="flex-1 min-w-0">
              <DialogTitle class="conn-dialog-title">
                {{ credentialDialogTitle }}
              </DialogTitle>
              <DialogDescription class="conn-dialog-desc">
                {{ t("credentials.description") }}
              </DialogDescription>
            </div>
            <DialogClose as-child>
              <button
                type="button"
                class="ui-icon-button shrink-0"
                :aria-label="t('actions.closeDialog')"
              >
                <X
                  :size="15"
                  stroke-width="1.8"
                />
              </button>
            </DialogClose>
          </header>

          <form
            class="conn-dialog-body"
            :autocomplete="NO_NATIVE_AUTOCOMPLETE"
            @submit.prevent="addCredential"
          >
            <div class="grid grid-cols-[repeat(2,minmax(0,1fr))] gap-[8px]">
              <button
                type="button"
                class="conn-protocol-card"
                :class="addingType === 'password' ? 'ui-nav-item-active' : ''"
                @click="selectCredentialType('password')"
              >
                <Lock
                  :size="14"
                  stroke-width="1.8"
                />
                {{ t("credentials.credTypes.password") }}
              </button>
              <button
                type="button"
                class="conn-protocol-card"
                :class="addingType === 'key' ? 'ui-nav-item-active' : ''"
                @click="selectCredentialType('key')"
              >
                <KeyRound
                  :size="14"
                  stroke-width="1.8"
                />
                {{ t("credentials.credTypes.key") }}
              </button>
            </div>

            <template v-if="addingType === 'password'">
              <label class="conn-field-group">
                <span class="conn-field-label">{{ t("credentials.fields.name") }}</span>
                <input
                  v-model="passwordForm.name"
                  class="ui-input"
                  :name="appFieldNames.credentialName"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.namePlaceholder')"
                >
              </label>
              <label class="conn-field-group">
                <span class="conn-field-label">{{ t("credentials.fields.password") }}</span>
                <input
                  v-model="passwordForm.password"
                  type="password"
                  class="ui-input"
                  :name="appFieldNames.password"
                  :placeholder="
                    editingId
                      ? t('credentials.fields.keepSecretPlaceholder')
                      : t('credentials.fields.passwordPlaceholder')
                  "
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                >
              </label>
            </template>

            <template v-else>
              <label class="conn-field-group">
                <span class="conn-field-label">{{ t("credentials.fields.name") }}</span>
                <input
                  v-model="keyForm.name"
                  class="ui-input"
                  :name="appFieldNames.credentialName"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.namePlaceholder')"
                >
              </label>
              <label class="conn-field-group">
                <span class="conn-field-label">{{ t("credentials.fields.passphrase") }}</span>
                <input
                  v-model="keyForm.passphrase"
                  type="password"
                  class="ui-input"
                  :name="appFieldNames.keyPassphrase"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.passphrasePlaceholder')"
                >
              </label>
              <label class="conn-field-group">
                <span class="conn-field-label">{{ t("credentials.fields.comment") }}</span>
                <input
                  v-model="keyForm.comment"
                  class="ui-input"
                  autocomplete="off"
                  :placeholder="t('credentials.fields.commentPlaceholder')"
                >
              </label>
              <div class="conn-field-group">
                <div class="conn-field-heading">
                  <span class="conn-field-label">{{ t("credentials.fields.privateKey") }}</span>
                  <button
                    type="button"
                    class="cred-file-btn"
                    @click.stop="pickPrivateKeyFile"
                  >
                    <FileKey
                      :size="12"
                      stroke-width="1.8"
                    />
                    {{ t("credentials.fields.readPrivateKeyFile") }}
                  </button>
                </div>
                <textarea
                  v-model="keyForm.privateKey"
                  class="ui-input conn-textarea"
                  :name="appFieldNames.privateKey"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="
                    editingId
                      ? t('credentials.fields.keepSecretPlaceholder')
                      : t('credentials.fields.privateKeyPlaceholder')
                  "
                  rows="3"
                />
              </div>
            </template>

            <span
              v-if="formError"
              class="conn-field-error"
            >{{ formError }}</span>
          </form>

          <footer class="conn-dialog-footer">
            <DialogClose as-child>
              <button
                type="button"
                class="ui-button-secondary"
              >
                {{ t("actions.cancel") }}
              </button>
            </DialogClose>
            <button
              type="button"
              class="ui-button-primary"
              :disabled="!canSave"
              @click="addCredential"
            >
              {{ t("actions.save") }}
            </button>
          </footer>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>

    <ConfirmDialog
      :open="credentialDeleteOpen"
      tone="danger"
      :loading="credentialDeleteBusy"
      :title="t('relationshipGraph.confirm.credentialDelete.title')"
      :description="pendingCredentialDeleteDescription"
      :confirm-text="t('relationshipGraph.confirm.credentialDelete.confirm')"
      :confirm-icon="Trash2"
      @update:open="onCredentialDeleteOpenChange"
      @confirm="confirmCredentialDelete"
    >
      <div
        v-if="pendingCredentialDelete?.usages?.length"
        class="relationship-delete-usage-list"
      >
        <span
          v-for="usage in pendingCredentialDelete.usages"
          :key="`${usage.connectionId}:${usage.relation}`"
          class="relationship-delete-usage-item"
          :title="`${usage.connectionName} · ${t(`credentials.relations.${usage.relation}`)}`"
        >
          <span class="relationship-delete-usage-name">{{ usage.connectionName }}</span>
          <span class="relationship-delete-usage-relation">
            {{ t(`credentials.relations.${usage.relation}`) }}
          </span>
        </span>
      </div>
    </ConfirmDialog>

    <ConfirmDialog
      v-model:open="cleanupConfirmOpen"
      tone="danger"
      :loading="isCleaning"
      :title="t('credentials.cleanup.confirmTitle')"
      :description="t('credentials.cleanup.confirmDescription', { count: unusedCount })"
      :confirm-text="t('credentials.cleanup.confirm')"
      :confirm-icon="Trash2"
      @confirm="confirmCleanupUnused"
    />

    <ConfirmDialog
      :open="typeChangeDialogOpen"
      tone="warning"
      :title="t('credentials.typeChangeConfirm.title')"
      :description="
        t('credentials.typeChangeConfirm.description', {
          count: typeChangeConfirm?.impact.affectedUsages.length || 0,
          names: typeChangeUsageNames(typeChangeConfirm?.impact.affectedUsages || []),
        })
      "
      :confirm-text="t('credentials.typeChangeConfirm.updateExisting')"
      :secondary-text="t('credentials.typeChangeConfirm.createNew')"
      :cancel-text="t('credentials.typeChangeConfirm.cancel')"
      @update:open="
        (open) => {
          if (!open) resolveTypeChangeConfirm(CREDENTIAL_TYPE_CHANGE_ACTION.CANCEL);
        }
      "
      @confirm="resolveTypeChangeConfirm(CREDENTIAL_TYPE_CHANGE_ACTION.UPDATE_EXISTING)"
      @secondary="resolveTypeChangeConfirm(CREDENTIAL_TYPE_CHANGE_ACTION.CREATE_NEW)"
    >
      <div
        v-if="typeChangeConfirm?.impact.affectedUsages.length"
        class="relationship-delete-usage-list"
      >
        <span
          v-for="usage in typeChangeConfirm.impact.affectedUsages"
          :key="`${usage.connectionId}:${usage.relation}`"
          class="relationship-delete-usage-item"
          :title="usage.connectionName || usage.connectionId"
        >
          <span class="relationship-delete-usage-name">
            {{ usage.connectionName || usage.connectionId }}
          </span>
          <span class="relationship-delete-usage-relation">
            {{ usage.protocol }}
          </span>
        </span>
      </div>
    </ConfirmDialog>
  </div>
</template>
