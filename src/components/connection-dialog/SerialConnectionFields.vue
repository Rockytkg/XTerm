<script setup>
import { RefreshCw } from "@lucide/vue";
import { useI18n } from "vue-i18n";
import UiSelect from "../UiSelect.vue";
import PasswordCredentialFields from "./PasswordCredentialFields.vue";

const props = defineProps({
  form: { type: Object, required: true },
  errors: { type: Object, required: true },
  filteredCredentials: { type: Array, default: () => [] },
  selectedCredential: { type: Object, default: null },
  serialPortOptions: { type: Array, required: true },
  baudRateOptions: { type: Array, required: true },
  dataBitsOptions: { type: Array, required: true },
  stopBitsOptions: { type: Array, required: true },
  parityOptions: { type: Array, required: true },
  flowControlOptions: { type: Array, required: true },
  loading: { type: Boolean, default: false },
});

const emit = defineEmits([
  "auth-method-change",
  "clear-field",
  "credential-select",
  "refresh-serial-ports",
  "update-field",
]);
const { t } = useI18n();

function updateField(field, value) {
  emit("update-field", field, value);
}
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.name") }}</span>
    <input
      :value="props.form.name"
      class="ui-input ui-fill-inline"
      :class="errors.name ? 'conn-input-error' : ''"
      placeholder="console"
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

  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.serialPort") }}</span>
    <div class="conn-inline-control">
      <UiSelect
        :model-value="props.form.serialPort"
        class="ui-fill-inline"
        :options="serialPortOptions"
        :invalid="Boolean(errors.serialPort)"
        @update:model-value="updateField('serialPort', $event)"
        @change="emit('clear-field', 'serialPort')"
      />
      <button
        type="button"
        class="conn-icon-btn"
        :disabled="loading"
        :aria-label="t('connectionDialog.refreshSerialPorts')"
        @click="emit('refresh-serial-ports')"
      >
        <RefreshCw
          :size="14"
          stroke-width="1.8"
          :class="{ 'animate-spin': loading }"
        />
      </button>
    </div>
    <span
      v-if="errors.serialPort"
      class="conn-field-error"
    >{{ errors.serialPort }}</span>
  </div>

  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.baudRate") }}</span>
    <UiSelect
      :model-value="props.form.baudRate"
      class="ui-fill-inline"
      :options="baudRateOptions"
      :invalid="Boolean(errors.baudRate)"
      @update:model-value="updateField('baudRate', $event)"
      @change="emit('clear-field', 'baudRate')"
    />
    <span
      v-if="errors.baudRate"
      class="conn-field-error"
    >{{ errors.baudRate }}</span>
  </div>

  <div class="conn-serial-line-grid">
    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.fields.dataBits") }}</span>
      <UiSelect
        :model-value="props.form.dataBits"
        class="ui-fill-inline"
        :options="dataBitsOptions"
        :invalid="Boolean(errors.dataBits)"
        @update:model-value="updateField('dataBits', $event)"
        @change="emit('clear-field', 'dataBits')"
      />
      <span
        v-if="errors.dataBits"
        class="conn-field-error"
      >{{ errors.dataBits }}</span>
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.fields.parity") }}</span>
      <UiSelect
        :model-value="props.form.parity"
        class="ui-fill-inline"
        :options="parityOptions"
        :invalid="Boolean(errors.parity)"
        @update:model-value="updateField('parity', $event)"
        @change="emit('clear-field', 'parity')"
      />
      <span
        v-if="errors.parity"
        class="conn-field-error"
      >{{ errors.parity }}</span>
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.fields.stopBits") }}</span>
      <UiSelect
        :model-value="props.form.stopBits"
        class="ui-fill-inline"
        :options="stopBitsOptions"
        :invalid="Boolean(errors.stopBits)"
        @update:model-value="updateField('stopBits', $event)"
        @change="emit('clear-field', 'stopBits')"
      />
      <span
        v-if="errors.stopBits"
        class="conn-field-error"
      >{{ errors.stopBits }}</span>
    </div>

    <div class="conn-field-group">
      <span class="conn-field-label">{{ t("connectionDialog.fields.flowControl") }}</span>
      <UiSelect
        :model-value="props.form.flowControl"
        class="ui-fill-inline"
        :options="flowControlOptions"
        :invalid="Boolean(errors.flowControl)"
        @update:model-value="updateField('flowControl', $event)"
        @change="emit('clear-field', 'flowControl')"
      />
      <span
        v-if="errors.flowControl"
        class="conn-field-error"
      >{{ errors.flowControl }}</span>
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
</template>
