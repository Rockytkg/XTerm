<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { Lock, ShieldCheck } from "@lucide/vue";
import UiSelect from "../UiSelect.vue";
import { appFieldNames, NO_NATIVE_AUTOCOMPLETE } from "../../utils/autocomplete";

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
  "update-field",
]);

const { t } = useI18n();
const credentialMode = computed(() =>
  props.form.savedCredentialId ? "saved" : "password",
);
const selectedSavedCredentialMissing = computed(
  () =>
    credentialMode.value === "saved" && props.form.savedCredentialId && !props.selectedCredential,
);
const credentialOptions = computed(() =>
  props.filteredCredentials.map((credential) => ({
    label: `${credential.name} · ${t(`credentials.credTypes.${credential.credType}`)}`,
    value: credential.id,
  })),
);

function updateField(field, value) {
  emit("update-field", field, value);
}

function selectCredentialMode(mode) {
  if (!mode) return;
  if (mode === "saved") {
    updateField("password", "");
    const credentialId = props.filteredCredentials[0]?.id;
    if (credentialId) emit("credential-select", credentialId);
    return;
  }
  emit("auth-method-change", "password");
}
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.authMethod") }}</span>
    <ToggleGroupRoot
      :model-value="credentialMode"
      type="single"
      class="conn-seg-tabs"
      @update:model-value="selectCredentialMode"
    >
      <ToggleGroupItem
        value="password"
        class="conn-seg-tab"
      >
        <Lock
          :size="11"
          stroke-width="2"
        />
        {{ t("connectionDialog.authMethods.password") }}
      </ToggleGroupItem>
      <ToggleGroupItem
        value="saved"
        class="conn-seg-tab"
      >
        <ShieldCheck
          :size="11"
          stroke-width="2"
        />
        {{ t("connectionDialog.authMethods.savedCredential") }}
      </ToggleGroupItem>
    </ToggleGroupRoot>
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
      v-if="credentialMode === 'saved'"
      class="conn-field-group"
    >
      <span class="conn-field-label">{{ t("connectionDialog.fields.savedCredential") }}</span>
      <UiSelect
        v-if="filteredCredentials.length"
        :model-value="form.savedCredentialId"
        class="ui-fill-inline"
        :options="credentialOptions"
        :invalid="Boolean(errors.savedCredentialId)"
        @update:model-value="emit('credential-select', $event)"
        @change="emit('clear-field', 'savedCredentialId')"
      />
      <div
        v-else
        class="conn-saved-empty"
      >
        {{ t("connectionDialog.noPasswordCredentials") }}
      </div>
      <span
        v-if="errors.savedCredentialId"
        class="conn-field-error"
      >{{
        errors.savedCredentialId
      }}</span>
      <span
        v-else-if="selectedSavedCredentialMissing"
        class="conn-field-error"
      >{{
        t("connectionDialog.validation.credentialNotFound")
      }}</span>
    </div>

    <div
      v-else
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
  </div>
</template>
