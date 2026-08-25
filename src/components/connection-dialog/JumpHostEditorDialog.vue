<script setup>
import "../../styles/connection-dialog.scss";
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  ListboxItem,
  ListboxItemIndicator,
  ListboxRoot,
  ToggleGroupItem,
  ToggleGroupRoot,
} from "reka-ui";
import {
  Check,
  ChevronDown,
  ChevronUp,
  FileKey,
  KeyRound,
  Network,
  Plus,
  Server,
  Trash2,
  X,
} from "@lucide/vue";
import { usePrivateKeyPicker } from "../../composables/usePrivateKeyPicker";
import { appFieldNames, NO_NATIVE_AUTOCOMPLETE } from "../../utils/autocomplete";
import { requiresHostKeyVerification } from "../../utils/connectionProtocols";
import UiSelect from "../UiSelect.vue";
import CredentialModeTabs from "./CredentialModeTabs.vue";
import { toCredentialOptions } from "./connectionDialogModel";

const props = defineProps({
  open: { type: Boolean, default: false },
  jumpHosts: { type: Array, default: () => [] },
  connections: { type: Array, default: () => [] },
  credentials: { type: Array, default: () => [] },
  currentConnectionId: { type: String, default: "" },
  error: { type: String, default: "" },
});

const emit = defineEmits(["update:open", "update:jumpHosts"]);
const { t } = useI18n();
const selectedIndex = ref(0);
const { pickPrivateKey, keepDialogOpen: keepDialogOpenForPrivateKeyPicker } =
  usePrivateKeyPicker();

const hops = computed(() => (Array.isArray(props.jumpHosts) ? props.jumpHosts : []));
const sshConnections = computed(() =>
  props.connections.filter((connection) => {
    const protocol = (connection.protocol || "ssh").toLowerCase();
    return requiresHostKeyVerification(protocol) && connection.id !== props.currentConnectionId;
  }),
);
const selectedHop = computed(() => hops.value[selectedIndex.value] ?? null);
const selectedConnection = computed(() => connectionById(selectedHop.value?.connectionId));
const hasCredentials = computed(() => props.credentials.length > 0);
const credentialOptions = computed(() => toCredentialOptions(props.credentials, t));
const manualAuthMode = computed(() => {
  if (selectedHop.value?.savedCredentialId) return "saved";
  return selectedHop.value?.authMethod || "password";
});

watch(
  () => hops.value.length,
  (length) => {
    if (!length) {
      selectedIndex.value = 0;
    } else if (selectedIndex.value >= length) {
      selectedIndex.value = length - 1;
    }
  },
);

function connectionById(id) {
  if (!id) return null;
  return sshConnections.value.find((connection) => connection.id === id) ?? null;
}

function connectionSummary(connection) {
  if (!connection) return "";
  const host = connection.host || "-";
  const user = connection.user ? `${connection.user}@` : "";
  const port = connection.port ? `:${connection.port}` : "";
  return `${user}${host}${port}`;
}

function hopMode(hop) {
  if (!hop) return "connection";
  return hop.source || (hop.connectionId ? "connection" : "manual");
}

function hopTitle(hop, index) {
  if (hop?.connectionId) {
    return (
      connectionById(hop.connectionId)?.name ||
      t("connectionDialog.jumpHostsEditor.missingConnection")
    );
  }
  if (hopMode(hop) === "connection") {
    return t("connectionDialog.jumpHostsEditor.chooseConnection");
  }
  if (hop?.host) return hop.host;
  return t("connectionDialog.fields.jumpHop", { index: index + 1 });
}

function hopSubtitle(hop) {
  if (hop?.connectionId) {
    return connectionSummary(connectionById(hop.connectionId));
  }
  if (hopMode(hop) === "connection") {
    return t("connectionDialog.jumpHostsEditor.existingConnection");
  }
  const credential = props.credentials.find((item) => item.id === hop?.savedCredentialId);
  const user = hop?.user ? `${hop.user}@` : "";
  const host = hop?.host || t("connectionDialog.jumpHostsEditor.manualHost");
  const port = hop?.port ? `:${hop.port}` : "";
  return credential ? `${user}${host}${port} · ${credential.name}` : `${user}${host}${port}`;
}

function emitHops(next) {
  emit("update:jumpHosts", next);
}

function newHop() {
  return {
    source: "connection",
    connectionId: "",
    host: "",
    port: "",
    user: "",
    authMethod: "",
    savedCredentialId: "",
    password: "",
    privateKey: "",
    keyPassphrase: "",
  };
}

function addHop() {
  const next = [...hops.value, newHop()];
  emitHops(next);
  selectedIndex.value = next.length - 1;
}

function updateHop(index, patch) {
  emitHops(hops.value.map((hop, hopIndex) => (hopIndex === index ? { ...hop, ...patch } : hop)));
}

function setHopSource(index, source) {
  if (!source) return;
  if (source === "connection") {
    updateHop(index, {
      source,
      connectionId: "",
      host: "",
      port: "",
      user: "",
      authMethod: "",
      savedCredentialId: "",
      password: "",
      privateKey: "",
      keyPassphrase: "",
    });
    return;
  }
  updateHop(index, {
    source,
    connectionId: "",
    authMethod: "password",
    savedCredentialId: "",
    password: "",
    privateKey: "",
    keyPassphrase: "",
  });
}

function selectConnection(index, connectionId) {
  updateHop(index, {
    source: "connection",
    connectionId,
    host: "",
    port: "",
    user: "",
    authMethod: "",
    savedCredentialId: "",
    password: "",
    privateKey: "",
    keyPassphrase: "",
  });
}

function selectCredential(index, credential) {
  if (!credential) return;
  updateHop(index, {
    savedCredentialId: credential.id,
    authMethod: credential.credType,
    password: "",
    privateKey: "",
    keyPassphrase: "",
  });
}

function setManualAuthMethod(index, method) {
  if (!method) return;
  const hop = hops.value[index];
  if (!hop) return;
  const methodChanged = hop.authMethod !== method;
  updateHop(index, {
    authMethod: method,
    savedCredentialId: "",
    password: methodChanged ? "" : hop.password || "",
    privateKey: methodChanged ? "" : hop.privateKey || "",
    keyPassphrase: methodChanged ? "" : hop.keyPassphrase || "",
  });
}

function removeHop(index) {
  const next = hops.value.filter((_, hopIndex) => hopIndex !== index);
  emitHops(next);
  selectedIndex.value = Math.min(index, Math.max(next.length - 1, 0));
}

function moveHop(index, offset) {
  const target = index + offset;
  if (target < 0 || target >= hops.value.length) return;
  const next = hops.value.slice();
  [next[index], next[target]] = [next[target], next[index]];
  emitHops(next);
  selectedIndex.value = target;
}

async function pickPrivateKeyFile(index) {
  const privateKey = await pickPrivateKey(t("credentials.fields.choosePrivateKeyTitle"));
  if (privateKey) {
    updateHop(index, { privateKey });
  }
}
</script>

<template>
  <DialogRoot
    :open="open"
    @update:open="emit('update:open', $event)"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-jump-dialog-overlay" />
      <DialogContent
        class="dialog-content conn-jump-dialog focus:outline-none"
        @pointer-down-outside="keepDialogOpenForPrivateKeyPicker"
        @interact-outside="keepDialogOpenForPrivateKeyPicker"
      >
        <form
          class="contents"
          :autocomplete="NO_NATIVE_AUTOCOMPLETE"
          @submit.prevent
        >
          <header class="conn-jump-dialog-header">
            <div
              class="conn-dialog-header-icon"
              aria-hidden="true"
            >
              <Network
                :size="16"
                stroke-width="1.8"
              />
            </div>
            <div class="min-w-0 flex-1">
              <DialogTitle class="conn-dialog-title">
                {{ t("connectionDialog.jumpHostsEditor.title") }}
              </DialogTitle>
              <DialogDescription class="conn-dialog-desc">
                {{ t("connectionDialog.jumpHostsEditor.description") }}
              </DialogDescription>
            </div>
            <DialogClose as-child>
              <button
                type="button"
                class="ui-icon-button"
                :aria-label="t('actions.closeDialog')"
              >
                <X
                  :size="15"
                  stroke-width="1.8"
                />
              </button>
            </DialogClose>
          </header>

          <div class="conn-jump-dialog-body">
            <aside class="conn-jump-chain-pane">
              <div class="conn-jump-chain-heading">
                <span>{{ t("connectionDialog.jumpHostsEditor.chain") }}</span>
                <button
                  type="button"
                  class="ui-row-action"
                  @click="addHop"
                >
                  <Plus
                    :size="13"
                    stroke-width="1.8"
                  />
                  {{ t("actions.add") }}
                </button>
              </div>

              <div
                v-if="!hops.length"
                class="conn-jump-editor-empty"
              >
                {{ t("connectionDialog.jumpHostsEditor.emptyChain") }}
              </div>

              <ListboxRoot
                v-else
                :model-value="selectedIndex"
                selection-behavior="replace"
                class="conn-jump-chain-list"
                @update:model-value="selectedIndex = Number($event)"
              >
                <ListboxItem
                  v-for="(hop, index) in hops"
                  :key="index"
                  :value="index"
                  class="conn-jump-chain-item"
                >
                  <div class="conn-jump-chain-select">
                    <span class="conn-jump-chain-step">{{ index + 1 }}</span>
                    <span class="conn-jump-chain-copy">
                      <span class="conn-jump-chain-title">{{ hopTitle(hop, index) }}</span>
                      <span class="conn-jump-chain-subtitle">{{ hopSubtitle(hop) }}</span>
                    </span>
                  </div>
                  <div class="conn-jump-chain-actions">
                    <button
                      type="button"
                      class="ui-row-action"
                      :disabled="index === 0"
                      @click.stop="moveHop(index, -1)"
                    >
                      <ChevronUp
                        :size="13"
                        stroke-width="1.8"
                      />
                    </button>
                    <button
                      type="button"
                      class="ui-row-action"
                      :disabled="index === hops.length - 1"
                      @click.stop="moveHop(index, 1)"
                    >
                      <ChevronDown
                        :size="13"
                        stroke-width="1.8"
                      />
                    </button>
                    <button
                      type="button"
                      class="ui-row-action ui-row-action-danger"
                      @click.stop="removeHop(index)"
                    >
                      <Trash2
                        :size="13"
                        stroke-width="1.8"
                      />
                    </button>
                  </div>
                </ListboxItem>
              </ListboxRoot>
            </aside>

            <section class="conn-jump-detail-pane">
              <template v-if="selectedHop">
                <div class="conn-jump-detail-heading">
                  <div>
                    <span class="conn-field-label">
                      {{ t("connectionDialog.fields.jumpHop", { index: selectedIndex + 1 }) }}
                    </span>
                    <span class="conn-field-hint">
                      {{ t("connectionDialog.jumpHostsEditor.detailHint") }}
                    </span>
                  </div>
                </div>

                <ToggleGroupRoot
                  :model-value="hopMode(selectedHop)"
                  type="single"
                  class="conn-seg-tabs"
                  @update:model-value="setHopSource(selectedIndex, $event)"
                >
                  <ToggleGroupItem
                    value="connection"
                    class="conn-seg-tab"
                  >
                    <Server
                      :size="11"
                      stroke-width="2"
                    />
                    {{ t("connectionDialog.jumpHostsEditor.existingConnection") }}
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="manual"
                    class="conn-seg-tab"
                  >
                    <KeyRound
                      :size="11"
                      stroke-width="2"
                    />
                    {{ t("connectionDialog.jumpHostsEditor.manualHost") }}
                  </ToggleGroupItem>
                </ToggleGroupRoot>

                <template v-if="hopMode(selectedHop) === 'connection'">
                  <div
                    v-if="!sshConnections.length"
                    class="conn-jump-editor-empty"
                  >
                    {{ t("connectionDialog.jumpHostsEditor.noConnections") }}
                  </div>
                  <ListboxRoot
                    v-else
                    :model-value="selectedHop.connectionId"
                    selection-behavior="replace"
                    class="conn-jump-picker-list"
                    @update:model-value="selectConnection(selectedIndex, $event)"
                  >
                    <ListboxItem
                      v-for="connection in sshConnections"
                      :key="connection.id"
                      :value="connection.id"
                      as="button"
                      type="button"
                      class="conn-jump-picker-card"
                    >
                      <span class="conn-jump-picker-icon">
                        <Server
                          :size="16"
                          stroke-width="1.8"
                        />
                      </span>
                      <span class="conn-jump-picker-main">
                        <span class="conn-jump-picker-name">{{ connection.name }}</span>
                        <span class="conn-jump-picker-meta">{{
                          connectionSummary(connection)
                        }}</span>
                      </span>
                      <ListboxItemIndicator class="shrink-0 text-accent">
                        <Check
                          :size="14"
                          stroke-width="2"
                        />
                      </ListboxItemIndicator>
                    </ListboxItem>
                  </ListboxRoot>
                  <div
                    v-if="selectedConnection"
                    class="conn-jump-selected-note"
                  >
                    {{ t("connectionDialog.jumpHostsEditor.inheritsConnection") }}
                  </div>
                </template>

                <template v-else>
                  <div class="conn-host-row">
                    <div class="conn-field-group conn-host-col">
                      <span class="conn-field-label">{{
                        t("connectionDialog.fields.jumpHost")
                      }}</span>
                      <input
                        :value="selectedHop.host"
                        class="ui-input ui-fill-inline"
                        placeholder="bastion.example.com"
                        @input="updateHop(selectedIndex, { host: $event.target.value })"
                      >
                    </div>
                    <div class="conn-field-group conn-port-col">
                      <span class="conn-field-label">{{ t("connectionDialog.fields.port") }}</span>
                      <input
                        :value="selectedHop.port"
                        inputmode="numeric"
                        pattern="[0-9]*"
                        class="ui-input ui-input-port ui-fill-inline"
                        placeholder="22"
                        autocomplete="off"
                        @input="
                          updateHop(selectedIndex, {
                            port: $event.target.value.replace(/\D/g, '').slice(0, 5),
                          })
                        "
                      >
                    </div>
                  </div>

                  <div class="conn-field-group">
                    <span class="conn-field-label">{{
                      t("connectionDialog.fields.authMethod")
                    }}</span>
                    <CredentialModeTabs
                      :model-value="manualAuthMode"
                      :show-saved="hasCredentials"
                      :methods="['password', 'key']"
                      @select="
                        $event === 'saved'
                          ? selectCredential(selectedIndex, credentials[0])
                          : setManualAuthMethod(selectedIndex, $event)
                      "
                    />
                  </div>

                  <div class="conn-field-group">
                    <span class="conn-field-label">{{ t("connectionDialog.fields.user") }}</span>
                    <input
                      :value="selectedHop.user"
                      class="ui-input ui-fill-inline"
                      :name="appFieldNames.user"
                      placeholder="root"
                      :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                      @input="updateHop(selectedIndex, { user: $event.target.value })"
                    >
                  </div>

                  <div
                    v-if="manualAuthMode === 'saved'"
                    class="conn-field-group"
                  >
                    <span class="conn-field-label">{{
                      t("connectionDialog.fields.savedCredential")
                    }}</span>
                    <UiSelect
                      v-if="hasCredentials"
                      :model-value="selectedHop.savedCredentialId"
                      class="ui-fill-inline"
                      :options="credentialOptions"
                      @update:model-value="
                        selectCredential(
                          selectedIndex,
                          credentials.find((credential) => credential.id === $event),
                        )
                      "
                    />
                    <span class="conn-field-hint">
                      {{ t("connectionDialog.jumpHostsEditor.manualCredentialHint") }}
                    </span>
                  </div>

                  <div
                    v-if="manualAuthMode === 'password'"
                    class="conn-field-group"
                  >
                    <span class="conn-field-label">{{
                      t("connectionDialog.fields.password")
                    }}</span>
                    <input
                      :value="selectedHop.password"
                      type="password"
                      :name="appFieldNames.password"
                      class="ui-input ui-fill-inline"
                      :placeholder="t('credentials.fields.passwordPlaceholder')"
                      :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                      @input="updateHop(selectedIndex, { password: $event.target.value })"
                    >
                  </div>

                  <template v-if="manualAuthMode === 'key'">
                    <div class="conn-field-group">
                      <span class="conn-field-label">{{
                        t("connectionDialog.fields.keyPassphrase")
                      }}</span>
                      <input
                        :value="selectedHop.keyPassphrase"
                        type="password"
                        :name="appFieldNames.keyPassphrase"
                        class="ui-input ui-fill-inline"
                        :placeholder="t('connectionDialog.fields.keyPassphrasePlaceholder')"
                        :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                        @input="updateHop(selectedIndex, { keyPassphrase: $event.target.value })"
                      >
                    </div>
                    <div class="conn-field-group">
                      <div class="conn-field-heading">
                        <span class="conn-field-label">{{
                          t("credentials.fields.privateKey")
                        }}</span>
                        <button
                          type="button"
                          class="conn-browse-btn"
                          @click.stop="pickPrivateKeyFile(selectedIndex)"
                        >
                          <FileKey
                            :size="12"
                            stroke-width="1.8"
                          />
                          {{ t("credentials.fields.readPrivateKeyFile") }}
                        </button>
                      </div>
                      <textarea
                        :value="selectedHop.privateKey"
                        class="ui-input conn-textarea ui-fill-inline"
                        :placeholder="t('credentials.fields.privateKeyPlaceholder')"
                        rows="4"
                        @input="updateHop(selectedIndex, { privateKey: $event.target.value })"
                      />
                    </div>
                  </template>
                </template>
              </template>

              <div
                v-else
                class="conn-jump-editor-empty conn-jump-detail-empty"
              >
                {{ t("connectionDialog.jumpHostsEditor.emptyDetail") }}
              </div>
            </section>
          </div>

          <footer class="conn-jump-dialog-footer">
            <span
              v-if="error"
              class="conn-field-error mr-auto"
            >{{ error }}</span>
            <DialogClose as-child>
              <button
                type="button"
                class="ui-button-primary"
              >
                {{ t("actions.done") }}
              </button>
            </DialogClose>
          </footer>
        </form>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
