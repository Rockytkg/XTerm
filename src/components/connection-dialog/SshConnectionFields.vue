<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { FileKey, Route } from "@lucide/vue";
import UiSelect from "../UiSelect.vue";
import CredentialModeTabs from "./CredentialModeTabs.vue";
import { toCredentialOptions } from "./connectionDialogModel";
import { appFieldNames, NO_NATIVE_AUTOCOMPLETE } from "../../utils/autocomplete";

const props = defineProps({
  form: { type: Object, required: true },
  errors: { type: Object, required: true },
  filteredCredentials: { type: Array, default: () => [] },
  jumpHostError: { type: String, default: "" },
});

const emit = defineEmits([
  "auth-method-change",
  "clear-field",
  "credential-select",
  "normalize-port",
  "open-jump-editor",
  "pick-private-key",
  "port-input",
  "update-field",
]);

const { t } = useI18n();
const hasFilteredCredentials = computed(() => props.filteredCredentials.length > 0);
const credentialMode = computed(() =>
  props.form.savedCredentialId ? "saved" : props.form.authMethod,
);
const credentialOptions = computed(() => toCredentialOptions(props.filteredCredentials, t));

function updateField(field, value) {
  emit("update-field", field, value);
}

function selectCredentialMode(mode) {
  if (!mode) return;
  if (mode === "saved") {
    const credentialId = props.filteredCredentials[0]?.id;
    if (credentialId) emit("credential-select", credentialId);
    return;
  }
  emit("auth-method-change", mode);
}

function jumpHosts() {
  return Array.isArray(props.form.jumpHosts) ? props.form.jumpHosts : [];
}

const jumpHostCount = computed(
  () => jumpHosts().filter((hop) => hop?.connectionId?.trim?.() || hop?.host?.trim?.()).length,
);
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.name") }}</span>
    <input
      :value="props.form.name"
      class="ui-input ui-fill-inline"
      :class="errors.name ? 'conn-input-error' : ''"
      placeholder="server"
      @input="
        updateField('name', $event.target.value);
        emit('clear-field', 'name');
      "
    >
    <span
      v-if="errors.name"
      class="conn-field-error"
    >{{ errors.name }}</span>
  </div>

  <div class="conn-host-row">
    <div class="conn-field-group conn-host-col">
      <span class="conn-field-label">{{ t("connectionDialog.fields.host") }}</span>
      <input
        :value="props.form.host"
        class="ui-input ui-fill-inline"
        :class="errors.host ? 'conn-input-error' : ''"
        placeholder="server.example.com"
        @input="
          updateField('host', $event.target.value);
          emit('clear-field', 'host');
        "
      >
      <span
        v-if="errors.host"
        class="conn-field-error"
      >{{ errors.host }}</span>
    </div>
    <div class="conn-field-group conn-port-col">
      <span class="conn-field-label">{{ t("connectionDialog.fields.port") }}</span>
      <input
        :value="props.form.port"
        inputmode="numeric"
        pattern="[0-9]*"
        class="ui-input ui-input-port ui-fill-inline"
        :class="errors.port ? 'conn-input-error' : ''"
        placeholder="22"
        @blur="emit('normalize-port')"
        @input="
          updateField('port', $event.target.value);
          emit('port-input');
        "
      >
      <span
        v-if="errors.port"
        class="conn-field-error"
      >{{ errors.port }}</span>
    </div>
  </div>

  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.authMethod") }}</span>
    <CredentialModeTabs
      :model-value="credentialMode"
      :show-saved="hasFilteredCredentials"
      :methods="['password', 'key']"
      @select="selectCredentialMode"
    />
  </div>

  <div class="conn-auth-line-grid">
    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.fields.user") }}</span>
      <input
        :value="props.form.user"
        :name="appFieldNames.user"
        class="ui-input ui-fill-inline"
        :class="errors.user ? 'conn-input-error' : ''"
        placeholder="root"
        :autocomplete="NO_NATIVE_AUTOCOMPLETE"
        @input="
          updateField('user', $event.target.value);
          emit('clear-field', 'user');
        "
      >
      <span
        v-if="errors.user"
        class="conn-field-error"
      >{{ errors.user }}</span>
    </div>

    <div
      v-if="credentialMode === 'saved' && hasFilteredCredentials"
      class="conn-field-group"
    >
      <span class="conn-field-label">{{ t("connectionDialog.fields.savedCredential") }}</span>
      <UiSelect
        :model-value="form.savedCredentialId"
        class="ui-fill-inline"
        :options="credentialOptions"
        :invalid="Boolean(errors.savedCredentialId)"
        @update:model-value="emit('credential-select', $event)"
        @change="emit('clear-field', 'savedCredentialId')"
      />
      <span
        v-if="errors.savedCredentialId"
        class="conn-field-error"
      >{{
        errors.savedCredentialId
      }}</span>
    </div>

    <div
      v-else-if="credentialMode === 'password'"
      class="conn-field-group"
    >
      <span class="conn-field-label">{{ t("connectionDialog.fields.password") }}</span>
      <input
        :value="props.form.password"
        type="password"
        :name="appFieldNames.password"
        class="ui-input ui-fill-inline"
        :class="errors.savedCredentialId ? 'conn-input-error' : ''"
        :placeholder="t('credentials.fields.passwordPlaceholder')"
        :autocomplete="NO_NATIVE_AUTOCOMPLETE"
        @input="
          updateField('password', $event.target.value);
          emit('clear-field', 'savedCredentialId');
        "
      >
    </div>

    <div
      v-else-if="credentialMode === 'key'"
      class="conn-field-group"
    >
      <span class="conn-field-label">{{ t("connectionDialog.fields.keyPassphrase") }}</span>
      <input
        :value="props.form.keyPassphrase"
        type="password"
        :name="appFieldNames.keyPassphrase"
        class="ui-input ui-fill-inline"
        :placeholder="t('connectionDialog.fields.keyPassphrasePlaceholder')"
        :autocomplete="NO_NATIVE_AUTOCOMPLETE"
        @input="updateField('keyPassphrase', $event.target.value)"
      >
    </div>
  </div>

  <div
    v-if="!hasFilteredCredentials"
    class="conn-field-group"
  >
    <span
      v-if="errors.savedCredentialId"
      class="conn-field-error"
    >{{
      errors.savedCredentialId
    }}</span>
    <span
      v-else-if="form.authMethod === 'key'"
      class="conn-field-hint"
    >
      {{ t("connectionDialog.validation.keyRequired") }}
    </span>
  </div>
  <span
    v-else-if="errors.savedCredentialId && credentialMode !== 'saved'"
    class="conn-field-error"
  >{{ errors.savedCredentialId }}</span>

  <template v-if="credentialMode === 'key'">
    <div class="conn-field-group">
      <div class="conn-field-heading">
        <span class="conn-field-label">{{ t("credentials.fields.privateKey") }}</span>
        <button
          type="button"
          class="conn-browse-btn"
          @click.stop="emit('pick-private-key')"
        >
          <FileKey
            :size="12"
            stroke-width="1.8"
          />
          {{ t("credentials.fields.readPrivateKeyFile") }}
        </button>
      </div>
      <textarea
        :value="props.form.privateKey"
        class="ui-input conn-textarea ui-fill-inline"
        :class="errors.savedCredentialId ? 'conn-input-error' : ''"
        :placeholder="t('credentials.fields.privateKeyPlaceholder')"
        rows="4"
        @input="
          updateField('privateKey', $event.target.value);
          emit('clear-field', 'savedCredentialId');
        "
      />
    </div>
  </template>

  <div class="conn-field-group">
    <div
      class="conn-jump-summary-row"
      :class="jumpHostError ? 'conn-input-error' : ''"
    >
      <div class="conn-jump-summary-main">
        <span
          class="conn-jump-summary-icon"
          :class="jumpHostError ? 'text-danger' : 'text-text-tertiary'"
        >
          <Route
            :size="15"
            stroke-width="1.8"
          />
        </span>
        <div class="min-w-0">
          <span class="conn-field-label">{{ t("connectionDialog.fields.jumpHosts") }}</span>
          <span class="conn-field-hint">
            {{
              jumpHostCount
                ? t("connectionDialog.fields.jumpHostsConfigured", { count: jumpHostCount })
                : t("connectionDialog.fields.jumpHostsHint")
            }}
          </span>
        </div>
      </div>
      <button
        type="button"
        class="conn-browse-btn"
        @click="emit('open-jump-editor')"
      >
        {{ t("connectionDialog.fields.manageJumpHosts") }}
      </button>
    </div>
    <span
      v-if="jumpHostError"
      class="conn-field-error"
    >{{ jumpHostError }}</span>
  </div>
</template>
