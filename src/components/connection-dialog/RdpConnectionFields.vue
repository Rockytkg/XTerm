<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import PasswordCredentialFields from "./PasswordCredentialFields.vue";
import UiSelect from "../UiSelect.vue";
import UiSwitch from "../UiSwitch.vue";

const props = defineProps({
  form: { type: Object, required: true },
  errors: { type: Object, required: true },
  filteredCredentials: { type: Array, default: () => [] },
  selectedCredential: { type: Object, default: null },
});

const emit = defineEmits([
  "auth-method-change",
  "clear-field",
  "credential-select",
  "normalize-port",
  "port-input",
  "update-field",
]);
const { t } = useI18n();

function updateField(field, value) {
  emit("update-field", field, value);
}

const scaleModeOptions = computed(() => [
  { label: t("connectionDialog.rdp.scaleModes.fit"), value: "fit" },
  { label: t("connectionDialog.rdp.scaleModes.none"), value: "none" },
  { label: t("connectionDialog.rdp.scaleModes.clip"), value: "clip" },
]);
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.name") }}</span>
    <input
      :value="props.form.name"
      class="ui-input ui-fill-inline"
      :class="errors.name ? 'conn-input-error' : ''"
      placeholder="rdp-desktop"
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
        placeholder="3389"
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

  <PasswordCredentialFields
    :form="props.form"
    :errors="errors"
    :filtered-credentials="filteredCredentials"
    :selected-credential="selectedCredential"
    @auth-method-change="emit('auth-method-change', $event)"
    @clear-field="emit('clear-field', $event)"
    @credential-select="emit('credential-select', $event)"
    @update-field="updateField"
  />

  <div class="conn-auth-line-grid">
    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.rdp.domain") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.rdp.domainHint") }}</span>
      <input
        :value="props.form.domain"
        class="ui-input ui-fill-inline"
        placeholder="WORKGROUP"
        @input="updateField('domain', $event.target.value)"
      >
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.rdp.scaleMode") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.rdp.scaleModeHint") }}</span>
      <UiSelect
        :model-value="props.form.scaleMode"
        :options="scaleModeOptions"
        @update:model-value="updateField('scaleMode', $event)"
      />
    </div>
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.rdp.clipboardSync") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.rdp.clipboardSyncHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.clipboardSync"
      @update:model-value="updateField('clipboardSync', $event)"
    />
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.rdp.resizeSession") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.rdp.resizeSessionHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.resizeSession"
      @update:model-value="updateField('resizeSession', $event)"
    />
  </div>
</template>
