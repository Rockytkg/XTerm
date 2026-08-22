<script setup>
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { Lock } from "@lucide/vue";
import { useI18n } from "vue-i18n";

const props = defineProps({
  modelValue: { type: String, required: true },
  locked: { type: Boolean, default: false },
  protocols: { type: Array, required: true },
});

const emit = defineEmits(["update:modelValue"]);
const { t } = useI18n();

function selectProtocol(protocol) {
  if (!protocol || props.locked || protocol === props.modelValue) return;
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
      :class="{ 'conn-protocol-grid-locked': locked }"
      :disabled="locked"
      @update:model-value="selectProtocol"
    >
      <ToggleGroupItem
        v-for="protocol in protocols"
        :key="protocol"
        :value="protocol"
        class="conn-protocol-card"
        :class="{ 'conn-protocol-card-hidden': locked && modelValue !== protocol }"
        :disabled="locked"
      >
        <span>{{ protocol.toUpperCase() }}</span>
        <Lock
          v-if="locked && modelValue === protocol"
          :size="12"
          stroke-width="1.9"
          class="conn-protocol-lock"
        />
      </ToggleGroupItem>
    </ToggleGroupRoot>
  </div>
</template>
