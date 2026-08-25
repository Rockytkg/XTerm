<script setup>
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { useI18n } from "vue-i18n";

const props = defineProps({
  modelValue: { type: String, required: true },
  protocols: { type: Array, required: true },
});

const emit = defineEmits(["update:modelValue"]);
const { t } = useI18n();

function selectProtocol(protocol) {
  if (!protocol || protocol === props.modelValue) return;
  emit("update:modelValue", protocol);
}
</script>

<template>
  <div class="conn-field-group">
    <span class="conn-field-label">{{ t("connectionDialog.fields.protocol") }}</span>
    <ToggleGroupRoot
      :model-value="modelValue"
      type="single"
      class="conn-protocol-grid"
      @update:model-value="selectProtocol"
    >
      <ToggleGroupItem
        v-for="protocol in protocols"
        :key="protocol"
        :value="protocol"
        class="conn-protocol-card"
      >
        {{ protocol.toUpperCase() }}
      </ToggleGroupItem>
    </ToggleGroupRoot>
  </div>
</template>
