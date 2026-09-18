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

const qualityOptions = computed(() =>
  Array.from({ length: 10 }, (_, level) => ({
    label: t(`connectionDialog.vnc.qualityLevels.${level}`),
    value: level,
  })),
);
const compressionOptions = computed(() =>
  Array.from({ length: 10 }, (_, level) => ({
    label: t(`connectionDialog.vnc.compressionLevels.${level}`),
    value: level,
  })),
);
const scaleModeOptions = computed(() => [
  { label: t("connectionDialog.vnc.scaleModes.fit"), value: "fit" },
  { label: t("connectionDialog.vnc.scaleModes.none"), value: "none" },
  { label: t("connectionDialog.vnc.scaleModes.clip"), value: "clip" },
]);
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.name") }}</span>
    <input
      :value="props.form.name"
      class="ui-input ui-fill-inline"
      :class="errors.name ? 'conn-input-error' : ''"
      placeholder="vnc-desktop"
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
        placeholder="5900"
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
      <span class="conn-field-label">{{ t("connectionDialog.vnc.scaleMode") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.scaleModeHint") }}</span>
      <UiSelect
        :model-value="props.form.scaleMode"
        :options="scaleModeOptions"
        @update:model-value="updateField('scaleMode', $event)"
      />
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.vnc.quality") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.qualityHint") }}</span>
      <UiSelect
        :model-value="Number(props.form.quality)"
        :options="qualityOptions"
        @update:model-value="updateField('quality', $event)"
      />
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.vnc.compression") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.compressionHint") }}</span>
      <UiSelect
        :model-value="Number(props.form.compression)"
        :options="compressionOptions"
        @update:model-value="updateField('compression', $event)"
      />
    </div>
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.vnc.clipboardSync") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.clipboardSyncHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.clipboardSync"
      @update:model-value="updateField('clipboardSync', $event)"
    />
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.vnc.viewOnly") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.viewOnlyHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.viewOnly"
      @update:model-value="updateField('viewOnly', $event)"
    />
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.vnc.shared") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.sharedHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.shared"
      @update:model-value="updateField('shared', $event)"
    />
  </div>

  <div class="conn-toggle-row">
    <div>
      <span class="conn-field-label">{{ t("connectionDialog.vnc.resizeSession") }}</span>
      <span class="conn-field-hint">{{ t("connectionDialog.vnc.resizeSessionHint") }}</span>
    </div>
    <UiSwitch
      :model-value="props.form.resizeSession"
      @update:model-value="updateField('resizeSession', $event)"
    />
  </div>
</template>
