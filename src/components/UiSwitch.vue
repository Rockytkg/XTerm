<script setup>
import { SwitchRoot, SwitchThumb } from "reka-ui";
import "../styles/toggle.scss";

defineProps({
  modelValue: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
});

defineEmits(["update:modelValue"]);
</script>

<template>
  <SwitchRoot
    class="ui-toggle"
    :model-value="modelValue"
    :disabled="disabled"
    @update:model-value="$emit('update:modelValue', $event)"
  >
    <SwitchThumb class="ui-toggle-thumb" />
  </SwitchRoot>
</template>
