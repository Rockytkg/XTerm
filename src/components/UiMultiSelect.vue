<script setup>
import "../styles/ui-select.scss";
import { computed, ref } from "vue";
import { Check, ChevronDown, Search } from "@lucide/vue";
import {
  ListboxContent,
  ListboxFilter,
  ListboxItem,
  ListboxItemIndicator,
  ListboxRoot,
  ListboxVirtualizer,
  PopoverContent,
  PopoverPortal,
  PopoverRoot,
  PopoverTrigger,
} from "reka-ui";

defineOptions({ inheritAttrs: false });

const props = defineProps({
  modelValue: { type: Array, default: () => [] },
  options: { type: Array, default: () => [] },
  placeholder: { type: String, default: "" },
  searchPlaceholder: { type: String, default: "" },
  emptyText: { type: String, default: "" },
  disabled: { type: Boolean, default: false },
});

const emit = defineEmits(["update:modelValue"]);

const searchTerm = ref("");

const selectedValues = computed(() => (Array.isArray(props.modelValue) ? props.modelValue : []));

const selectedLabel = computed(() =>
  props.options
    .filter((option) => selectedValues.value.includes(option.value))
    .map((option) => option.label)
    .join("、"),
);

const filteredOptions = computed(() => {
  const term = searchTerm.value.trim().toLowerCase();
  if (!term) return props.options;
  return props.options.filter((option) => String(option.label).toLowerCase().includes(term));
});

function updateValue(value) {
  emit("update:modelValue", Array.isArray(value) ? value : []);
}
</script>

<template>
  <div
    class="ui-select-custom"
    :class="$attrs.class"
  >
    <PopoverRoot>
      <PopoverTrigger as-child>
        <button
          type="button"
          class="ui-select-trigger ui-multiselect-trigger"
          :disabled="disabled"
        >
          <span
            class="ui-select-value"
            :class="{ 'ui-select-placeholder': !selectedLabel }"
          >
            {{ selectedLabel || placeholder }}
          </span>
          <span
            v-if="selectedValues.length"
            class="ui-multiselect-count"
          >
            {{ selectedValues.length }}
          </span>
          <ChevronDown
            :size="14"
            stroke-width="1.8"
            class="ui-select-chevron"
          />
        </button>
      </PopoverTrigger>
      <PopoverPortal>
        <PopoverContent
          class="ui-multiselect-menu"
          side="bottom"
          align="start"
          :side-offset="6"
          :collision-padding="8"
        >
          <ListboxRoot
            :model-value="selectedValues"
            multiple
            selection-behavior="toggle"
            highlight-on-hover
            @update:model-value="updateValue"
          >
            <div class="ui-multiselect-search">
              <Search
                :size="13"
                stroke-width="1.8"
                class="ui-multiselect-search-icon"
              />
              <ListboxFilter
                v-model="searchTerm"
                class="ui-multiselect-search-input"
                :placeholder="searchPlaceholder"
              />
            </div>
            <div
              v-if="!filteredOptions.length"
              class="ui-multiselect-empty"
            >
              {{ emptyText }}
            </div>
            <ListboxContent
              v-else
              class="ui-multiselect-list"
              :style="{ height: `${Math.min(filteredOptions.length * 34 + 8, 240)}px` }"
            >
              <ListboxVirtualizer
                v-slot="{ option }"
                :options="filteredOptions"
                :estimate-size="34"
              >
                <ListboxItem
                  :value="option.value"
                  class="ui-select-option"
                >
                  <span class="ui-multiselect-check">
                    <ListboxItemIndicator>
                      <Check
                        :size="13"
                        stroke-width="2"
                      />
                    </ListboxItemIndicator>
                  </span>
                  <span class="ui-select-option-label">{{ option.label }}</span>
                </ListboxItem>
              </ListboxVirtualizer>
            </ListboxContent>
          </ListboxRoot>
        </PopoverContent>
      </PopoverPortal>
    </PopoverRoot>
  </div>
</template>
