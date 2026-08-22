<script setup>
import "../styles/ui-select.scss";
import {
  SelectContent,
  SelectIcon,
  SelectItem,
  SelectItemIndicator,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectValue,
  SelectViewport,
} from "reka-ui";
import { Check, ChevronDown } from "@lucide/vue";
import { computed } from "vue";

defineOptions({ inheritAttrs: false });

const props = defineProps({
  modelValue: { type: [String, Number], default: "" },
  options: { type: Array, default: () => [] },
  placeholder: { type: String, default: "" },
  invalid: { type: Boolean, default: false },
  disabled: { type: Boolean, default: false },
});

const emit = defineEmits(["update:modelValue", "change"]);

const normalizedOptions = computed(() =>
  props.options.map((option) => {
    if (option && typeof option === "object") return option;
    return { label: String(option), value: option };
  }),
);

const selectedOption = computed(
  () => normalizedOptions.value.find((option) => option.value === props.modelValue) ?? null,
);

function updateValue(value) {
  emit("update:modelValue", value);
  emit("change", value);
}
</script>

<template>
  <div
    class="ui-select-custom"
    :class="$attrs.class"
  >
    <SelectRoot
      :model-value="modelValue"
      :disabled="disabled"
      @update:model-value="updateValue"
    >
      <SelectTrigger
        class="ui-select-trigger"
        :class="{ 'conn-input-error': invalid }"
      >
        <SelectValue
          class="ui-select-value"
          :class="{ 'ui-select-placeholder': !selectedOption }"
        >
          {{ selectedOption?.label ?? placeholder }}
        </SelectValue>
        <SelectIcon as-child>
          <ChevronDown
            :size="14"
            stroke-width="1.8"
            class="ui-select-chevron"
          />
        </SelectIcon>
      </SelectTrigger>

      <SelectPortal>
        <SelectContent
          class="ui-select-menu"
          position="popper"
          :side-offset="6"
        >
          <SelectViewport>
            <SelectItem
              v-for="option in normalizedOptions"
              :key="`${option.value}:${option.label}`"
              :value="option.value"
              :text-value="String(option.label ?? option.value ?? '')"
              class="ui-select-option"
            >
              <SelectItemText class="ui-select-option-label">
                {{ option.label }}
              </SelectItemText>
              <SelectItemIndicator>
                <Check
                  :size="13"
                  stroke-width="2"
                />
              </SelectItemIndicator>
            </SelectItem>
          </SelectViewport>
        </SelectContent>
      </SelectPortal>
    </SelectRoot>
  </div>
</template>
