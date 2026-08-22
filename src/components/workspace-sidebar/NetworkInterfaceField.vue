<script setup>
import { computed } from "vue";
import { RefreshCw } from "@lucide/vue";
import UiSelect from "../UiSelect.vue";

const props = defineProps({
  bindIp: { type: String, default: "" },
  disabled: { type: Boolean, default: false },
  hint: { type: String, required: true },
  label: { type: String, required: true },
  options: { type: Array, default: () => [] },
  refreshDisabled: { type: Boolean, default: false },
  refreshLabel: { type: String, required: true },
  refreshing: { type: Boolean, default: false },
});

const emit = defineEmits(["refresh", "update:bindIp"]);

const selectDisabled = computed(() => props.disabled || props.refreshDisabled);

function handleRefresh() {
  emit("refresh");
}

function handleUpdate(value) {
  emit("update:bindIp", value);
}
</script>

<template>
  <div class="workspace-sidebar-pref-row workspace-sidebar-pref-row-stack">
    <div class="workspace-sidebar-pref-head">
      <div class="workspace-sidebar-pref-text">
        <span class="workspace-sidebar-pref-label">{{ label }}</span>
        <span class="workspace-sidebar-pref-hint">{{ hint }}</span>
      </div>
      <button
        type="button"
        class="ui-icon-button shrink-0"
        :aria-label="refreshLabel"
        :disabled="refreshDisabled"
        :title="refreshLabel"
        @click="handleRefresh"
      >
        <RefreshCw
          :size="14"
          stroke-width="1.9"
          :class="{ 'animate-spin': refreshing }"
        />
      </button>
    </div>
    <UiSelect
      :model-value="bindIp"
      :options="options"
      :disabled="selectDisabled"
      @update:model-value="handleUpdate"
    />
  </div>
</template>
