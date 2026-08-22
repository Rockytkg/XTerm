<script setup>
import { computed, reactive, ref, watch } from "vue";
// Per-view import: this component renders independently from other consumers of this stylesheet.
import "../styles/settings-tab-switcher.scss";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
  ToggleGroupItem,
  ToggleGroupRoot,
} from "reka-ui";
import { Clock3, KeyRound, LockKeyhole, Save, X } from "@lucide/vue";
import { useI18n } from "vue-i18n";
import { choosePrivateKey } from "../services/credentials";
import ConfirmDialog from "./ConfirmDialog.vue";
import { appFieldNames, NO_NATIVE_AUTOCOMPLETE } from "../utils/autocomplete";
import { CREDENTIAL_TYPE_CHANGE_ACTION } from "../utils/credentialTypeChange";

const props = defineProps({
  prompt: { type: Object, required: true },
  busy: { type: Boolean, default: false },
});
const emit = defineEmits(["cancel", "submit"]);
const { t } = useI18n();

const form = reactive({
  authMethod: "password",
  username: "",
  password: "",
  privateKey: "",
  passphrase: "",
  credentialPersistence: "temporary",
});
const error = ref("");
const pickerBusy = ref(false);

const target = computed(() => {
  const connection = props.prompt.connection || {};
  const user = form.username.trim() || connection.user || "user";
  const host = connection.host || "host";
  const port = connection.port || "22";
  return `${user}@${host}:${port}`;
});
const saveAllowed = computed(() => props.prompt.saveAllowed !== false);
const persistenceOptions = computed(() => {
  if (!saveAllowed.value) return [];
  const options = [
    {
      label: t("sshCredentialPrompt.persistence.temporary"),
      value: "temporary",
    },
  ];
  if (props.prompt.canUpdateCredential) {
    options.push({
      label: t("sshCredentialPrompt.persistence.updateExisting"),
      value: "updateExisting",
    });
  }
  options.push({
    label:
      props.prompt.reason === "missing"
        ? t("sshCredentialPrompt.persistence.saveAndLink")
        : t("sshCredentialPrompt.persistence.createNew"),
    value: "createNew",
  });
  return options;
});
const willPersist = computed(() => saveAllowed.value && form.credentialPersistence !== "temporary");
const typeChangeConfirm = computed(() => props.prompt.typeChangeConfirm || null);
const authMethodModel = computed({
  get: () => form.authMethod,
  set: (method) => {
    if (!method || props.busy || form.authMethod === method) return;
    form.authMethod = method;
    error.value = "";
  },
});
const credentialPersistenceModel = computed({
  get: () => form.credentialPersistence,
  set: (value) => {
    if (!value || props.busy) return;
    form.credentialPersistence = value;
  },
});

watch(
  () => props.prompt,
  (prompt) => {
    const connection = prompt?.connection || {};
    form.authMethod = connection.authMethod || "password";
    form.username = connection.user || "";
    form.password = "";
    form.privateKey = "";
    form.passphrase = "";
    form.credentialPersistence = "temporary";
    error.value = "";
  },
  { immediate: true },
);

async function pickPrivateKey() {
  if (pickerBusy.value || props.busy) return;
  pickerBusy.value = true;
  try {
    const key = await choosePrivateKey(t("credentials.fields.choosePrivateKeyTitle"));
    if (key) {
      form.privateKey = key;
      error.value = "";
    }
  } finally {
    window.setTimeout(() => {
      pickerBusy.value = false;
    }, 250);
  }
}

function submit() {
  submitWithPersistence(form.credentialPersistence);
}

function submitWithPersistence(credentialPersistence) {
  const username = form.username.trim();
  if (!username) {
    error.value = t("sshCredentialPrompt.validation.usernameRequired");
    return;
  }
  if (form.authMethod === "password" && !form.password) {
    error.value = t("sshCredentialPrompt.validation.passwordRequired");
    return;
  }
  if (form.authMethod === "key" && !form.privateKey.trim()) {
    error.value = t("sshCredentialPrompt.validation.privateKeyRequired");
    return;
  }
  error.value = "";
  emit("submit", {
    authMethod: form.authMethod,
    username,
    password: form.authMethod === "password" ? form.password : "",
    privateKey: form.authMethod === "key" ? form.privateKey : "",
    passphrase: form.authMethod === "key" ? form.passphrase : "",
    credentialPersistence: saveAllowed.value ? credentialPersistence : "temporary",
  });
}

function confirmTypeChange(action) {
  const pending = props.prompt.pendingInput;
  if (!pending || action === CREDENTIAL_TYPE_CHANGE_ACTION.CANCEL) {
    emit("cancel");
    return;
  }
  emit("submit", {
    ...pending,
    credentialPersistence: action,
    typeChangeAction: action,
  });
}

function typeChangeUsageNames(usages) {
  return usages
    .map((usage) => usage.connectionName || usage.connectionId)
    .filter(Boolean)
    .join(", ");
}
</script>

<template>
  <DialogRoot :open="!typeChangeConfirm">
    <DialogPortal>
      <DialogOverlay class="dialog-overlay host-key-dialog-overlay" />
      <DialogContent
        class="dialog-content host-key-dialog ssh-credential-dialog"
        @escape-key-down.prevent
        @pointer-down-outside.prevent
        @interact-outside.prevent
      >
        <header class="host-key-dialog-header">
          <div
            class="host-key-dialog-icon ssh-credential-dialog-icon"
            aria-hidden="true"
          >
            <LockKeyhole
              :size="22"
              stroke-width="1.8"
            />
          </div>
          <div>
            <DialogTitle class="host-key-dialog-title">
              {{
                prompt.reason === "missing"
                  ? t("sshCredentialPrompt.missingTitle")
                  : t("sshCredentialPrompt.rejectedTitle")
              }}
            </DialogTitle>
            <DialogDescription class="host-key-dialog-desc">
              {{
                prompt.reason === "missing"
                  ? t("sshCredentialPrompt.missingDescription", { target })
                  : t("sshCredentialPrompt.rejectedDescription", { target })
              }}
            </DialogDescription>
          </div>
        </header>

        <div class="ssh-credential-scroll">
          <form
            class="ssh-credential-form"
            :autocomplete="NO_NATIVE_AUTOCOMPLETE"
            @submit.prevent="submit"
          >
            <div
              class="ssh-credential-inline-row"
              :class="form.authMethod === 'key' ? 'ssh-credential-inline-row-two' : ''"
            >
              <label>
                <span class="conn-field-label">{{ t("credentials.fields.username") }}</span>
                <input
                  v-model="form.username"
                  class="w-full"
                  :name="appFieldNames.user"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.usernamePlaceholder')"
                  :disabled="busy"
                >
              </label>
              <label v-if="form.authMethod === 'key'">
                <span class="conn-field-label">{{ t("credentials.fields.passphrase") }}</span>
                <input
                  v-model="form.passphrase"
                  class="w-full"
                  type="password"
                  :name="appFieldNames.keyPassphrase"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.passphrasePlaceholder')"
                  :disabled="busy"
                >
              </label>
            </div>

            <ToggleGroupRoot
              v-model="authMethodModel"
              type="single"
              class="conn-seg-tabs ssh-credential-methods"
              :aria-label="t('connectionDialog.fields.authMethod')"
            >
              <ToggleGroupItem
                value="password"
                class="conn-seg-tab"
                :disabled="busy"
              >
                <LockKeyhole
                  :size="11"
                  stroke-width="2"
                />
                {{ t("connectionDialog.authMethods.password") }}
              </ToggleGroupItem>
              <ToggleGroupItem
                value="key"
                class="conn-seg-tab"
                :disabled="busy"
              >
                <KeyRound
                  :size="11"
                  stroke-width="2"
                />
                {{ t("connectionDialog.authMethods.key") }}
              </ToggleGroupItem>
            </ToggleGroupRoot>

            <label v-if="form.authMethod === 'password'">
              <span class="conn-field-label">{{ t("credentials.fields.password") }}</span>
              <input
                v-model="form.password"
                class="w-full"
                type="password"
                :name="appFieldNames.password"
                :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                :placeholder="t('credentials.fields.passwordPlaceholder')"
                :disabled="busy"
              >
            </label>

            <template v-else>
              <label>
                <span class="conn-field-label">{{ t("credentials.fields.privateKey") }}</span>
                <textarea
                  v-model="form.privateKey"
                  rows="4"
                  class="w-full"
                  :name="appFieldNames.privateKey"
                  :autocomplete="NO_NATIVE_AUTOCOMPLETE"
                  :placeholder="t('credentials.fields.privateKeyPlaceholder')"
                  :disabled="busy"
                />
              </label>
              <button
                type="button"
                class="ui-button-secondary ssh-credential-file"
                :disabled="busy || pickerBusy"
                @click="pickPrivateKey"
              >
                <KeyRound
                  :size="14"
                  stroke-width="1.9"
                />
                {{ t("credentials.fields.readPrivateKeyFile") }}
              </button>
            </template>

            <div
              v-if="persistenceOptions.length"
              class="ssh-credential-persistence"
            >
              <span class="conn-field-label">{{ t("sshCredentialPrompt.persistence.label") }}</span>
              <ToggleGroupRoot
                v-model="credentialPersistenceModel"
                type="single"
                class="settings-tab-switcher ssh-credential-persistence-switch"
                :aria-label="t('sshCredentialPrompt.persistence.label')"
              >
                <ToggleGroupItem
                  v-for="option in persistenceOptions"
                  :key="option.value"
                  :value="option.value"
                  class="settings-tab-option ssh-credential-persistence-option"
                  :disabled="busy"
                >
                  {{ option.label }}
                </ToggleGroupItem>
              </ToggleGroupRoot>
            </div>

            <p
              v-if="error"
              class="ssh-credential-error"
            >
              {{ error }}
            </p>
          </form>
        </div>

        <footer class="host-key-dialog-actions">
          <button
            type="button"
            class="ui-button-secondary"
            :disabled="busy"
            @click="emit('cancel')"
          >
            <X
              :size="14"
              stroke-width="1.9"
            />
            {{ t("sshCredentialPrompt.cancel") }}
          </button>
          <button
            type="button"
            class="ui-button-primary host-key-save"
            :disabled="busy"
            @click="submit"
          >
            <Save
              v-if="willPersist"
              :size="14"
              stroke-width="1.9"
            />
            <Clock3
              v-else
              :size="14"
              stroke-width="1.9"
            />
            {{
              willPersist
                ? t(
                  form.credentialPersistence === "updateExisting"
                    ? "sshCredentialPrompt.updateConnect"
                    : "sshCredentialPrompt.saveConnect",
                )
                : t("sshCredentialPrompt.connectOnce")
            }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>

  <ConfirmDialog
    :open="!!typeChangeConfirm"
    tone="warning"
    :title="t('credentials.typeChangeConfirm.title')"
    :description="
      t('credentials.typeChangeConfirm.description', {
        count: typeChangeConfirm?.affectedUsages.length || 0,
        names: typeChangeUsageNames(typeChangeConfirm?.affectedUsages || []),
      })
    "
    :confirm-text="t('credentials.typeChangeConfirm.updateExisting')"
    :secondary-text="t('credentials.typeChangeConfirm.createNew')"
    :cancel-text="t('credentials.typeChangeConfirm.cancel')"
    @update:open="
      (open) => {
        if (!open) confirmTypeChange(CREDENTIAL_TYPE_CHANGE_ACTION.CANCEL);
      }
    "
    @confirm="confirmTypeChange(CREDENTIAL_TYPE_CHANGE_ACTION.UPDATE_EXISTING)"
    @secondary="confirmTypeChange(CREDENTIAL_TYPE_CHANGE_ACTION.CREATE_NEW)"
  >
    <div
      v-if="typeChangeConfirm?.affectedUsages.length"
      class="relationship-delete-usage-list"
    >
      <span
        v-for="usage in typeChangeConfirm.affectedUsages"
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
</template>
