<script setup>
import { computed } from "vue";
import { CollapsibleContent, CollapsibleRoot, CollapsibleTrigger } from "reka-ui";
import { ChevronDown, SlidersHorizontal } from "@lucide/vue";
import { useI18n } from "vue-i18n";
import UiSwitch from "../UiSwitch.vue";
import UiSelect from "../UiSelect.vue";
import { isSerialProtocol } from "../../utils/connectionProtocols";
import {
  BACKSPACE_SENDS_OPTIONS,
  TERMINAL_TYPE_OPTIONS,
  createEncodingOptions,
} from "../../utils/terminalSessionOptions";

const props = defineProps({
  form: { type: Object, required: true },
  open: { type: Boolean, default: false },
  protocol: { type: String, required: true },
});

const emit = defineEmits(["update-field", "update:open"]);
const { t } = useI18n();
const isSerial = computed(() => isSerialProtocol(props.protocol));

function updateField(field, value) {
  emit("update-field", field, value);
}

const encodingOptions = computed(() => createEncodingOptions(t));
</script>

<template>
  <CollapsibleRoot
    class="conn-advanced"
    :open="open"
    @update:open="emit('update:open', $event)"
  >
    <CollapsibleTrigger class="conn-advanced-summary">
      <span class="conn-advanced-summary-label">
        <SlidersHorizontal
          :size="14"
          stroke-width="1.8"
        />
        <span>{{ t("connectionDialog.fields.sessionOptions") }}</span>
      </span>
      <ChevronDown
        :size="14"
        stroke-width="1.8"
        class="conn-advanced-chevron"
      />
    </CollapsibleTrigger>

    <CollapsibleContent class="conn-advanced-content">
      <div class="conn-field-group">
        <span class="conn-field-label">{{ t("connectionDialog.fields.terminalType") }}</span>
        <span class="conn-field-hint">{{ t("connectionDialog.fields.terminalTypeHint") }}</span>
        <UiSelect
          :model-value="form.terminalType"
          :options="TERMINAL_TYPE_OPTIONS"
          @update:model-value="updateField('terminalType', $event)"
        />
      </div>

      <div class="conn-field-group">
        <span class="conn-field-label">{{ t("connectionDialog.fields.encoding") }}</span>
        <span class="conn-field-hint">{{ t("connectionDialog.fields.encodingHint") }}</span>
        <UiSelect
          :model-value="form.encoding"
          :options="encodingOptions"
          @update:model-value="updateField('encoding', $event)"
        />
      </div>

      <div class="conn-field-group">
        <span class="conn-field-label">{{ t("connectionDialog.fields.backspaceSends") }}</span>
        <span class="conn-field-hint">{{ t("connectionDialog.fields.backspaceSendsHint") }}</span>
        <UiSelect
          :model-value="form.backspaceSends"
          :options="BACKSPACE_SENDS_OPTIONS"
          @update:model-value="updateField('backspaceSends', $event)"
        />
      </div>

      <div class="conn-toggle-row">
        <div>
          <span class="conn-field-label">{{ t("connectionDialog.fields.highlightEnabled") }}</span>
          <span class="conn-field-hint">{{
            t("connectionDialog.fields.highlightEnabledHint")
          }}</span>
        </div>
        <UiSwitch
          :model-value="props.form.terminalHighlightEnabled"
          @update:model-value="updateField('terminalHighlightEnabled', $event)"
        />
      </div>

      <div class="conn-toggle-row">
        <div>
          <span class="conn-field-label">{{ t("connectionDialog.fields.morePromptCleanup") }}</span>
          <span class="conn-field-hint">{{
            t("connectionDialog.fields.morePromptCleanupHint")
          }}</span>
        </div>
        <UiSwitch
          :model-value="props.form.terminalMorePromptCleanup"
          @update:model-value="updateField('terminalMorePromptCleanup', $event)"
        />
      </div>

      <div
        v-if="isSerial"
        class="conn-toggle-row"
      >
        <div>
          <span class="conn-field-label">{{
            t("connectionDialog.fields.serialQuickAutoBaud")
          }}</span>
          <span class="conn-field-hint">{{
            t("connectionDialog.fields.serialQuickAutoBaudHint")
          }}</span>
        </div>
        <UiSwitch
          :model-value="props.form.serialQuickAutoBaud"
          @update:model-value="updateField('serialQuickAutoBaud', $event)"
        />
      </div>
    </CollapsibleContent>
  </CollapsibleRoot>
</template>
