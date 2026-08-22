<script setup>
import "../styles/font-picker.scss";
import {
  ComboboxAnchor,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxItemIndicator,
  ComboboxPortal,
  ComboboxRoot,
  ComboboxTrigger,
  ComboboxViewport,
} from "reka-ui";
import { Check, ChevronDown, Type } from "@lucide/vue";

const props = defineProps({
  modelValue: { type: String, default: "" },
  fonts: { type: Array, default: () => [] },
  placeholder: { type: String, default: "Select font..." },
  noResultsText: { type: String, default: "No fonts found" },
});

const emit = defineEmits(["update:modelValue"]);

function displayFont(value) {
  return typeof value === "string" ? value : "";
}

function updateFont(value) {
  if (typeof value === "string") {
    emit("update:modelValue", value);
  }
}
</script>

<template>
  <ComboboxRoot
    :model-value="modelValue"
    open-on-click
    open-on-focus
    reset-search-term-on-blur
    reset-search-term-on-select
    @update:model-value="updateFont"
  >
    <ComboboxAnchor class="font-picker">
      <div class="font-picker-trigger">
        <Type
          :size="15"
          stroke-width="1.8"
        />
        <ComboboxInput
          class="font-picker-input"
          :display-value="displayFont"
          :placeholder="props.placeholder"
        />
        <ComboboxTrigger class="font-picker-chevron-button">
          <ChevronDown
            :size="14"
            stroke-width="1.8"
            class="font-picker-chevron"
          />
        </ComboboxTrigger>
      </div>
    </ComboboxAnchor>

    <ComboboxPortal>
      <ComboboxContent
        class="font-picker-menu"
        position="popper"
        align="start"
        :side-offset="6"
        :collision-padding="10"
      >
        <ComboboxViewport class="font-picker-list">
          <ComboboxItem
            v-for="font in fonts"
            :key="font"
            :value="font"
            :text-value="font"
            class="font-picker-option"
          >
            <span
              class="font-picker-sample"
              :style="{ '--font-picker-sample-font': `'${font}', monospace` }"
            >Aa</span>
            <span class="font-picker-name">{{ font }}</span>
            <ComboboxItemIndicator class="font-picker-check">
              <Check
                :size="13"
                stroke-width="2"
              />
            </ComboboxItemIndicator>
          </ComboboxItem>
          <ComboboxEmpty class="font-picker-empty">
            {{ noResultsText }}
          </ComboboxEmpty>
        </ComboboxViewport>
      </ComboboxContent>
    </ComboboxPortal>
  </ComboboxRoot>
</template>
