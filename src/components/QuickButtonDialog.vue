<script setup>
import { computed, reactive, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { Zap } from "@lucide/vue";
import UiSelect from "./UiSelect.vue";
import { useScriptsStore } from "../stores/scriptsStore";
import { DEFAULT_COLOR, useQuickButtons } from "../composables/useQuickButtons";

const props = defineProps({ open: Boolean, button: { type: Object, default: null } });
const emit = defineEmits(["update:open"]);
const scriptsStore = useScriptsStore();
const { upsert } = useQuickButtons();
const { t } = useI18n();

const form = reactive({ id: "", name: "", type: "send", value: "", color: DEFAULT_COLOR });
const presetColors = [
  "#4f8cff",
  "#35c98a",
  "#f2b84b",
  "#ef6b73",
  "#b47cff",
  "#45c7d9",
  "#f28c52",
  "#e8edf5",
];

const behaviorOptions = computed(() => [
  { label: t("statusBar.quickButtons.send"), value: "send" },
  { label: t("statusBar.quickButtons.script"), value: "script" },
]);
const scriptOptions = computed(() =>
  scriptsStore.scripts.map((script) => ({ label: script.name, value: script.id })),
);
const canSubmit = computed(() => {
  if (!form.name.trim()) return false;
  if (form.type === "script") {
    // 脚本可能被删除，id 必须仍存在于脚本库才允许保存
    return scriptOptions.value.some((option) => option.value === form.value);
  }
  return Boolean(form.value);
});

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    Object.assign(
      form,
      props.button || { id: "", name: "", type: "send", value: "", color: DEFAULT_COLOR },
    );
  },
);

function submit() {
  if (!canSubmit.value) return;
  upsert({ ...form, name: form.name.trim() });
  emit("update:open", false);
}
</script>

<template>
  <DialogRoot
    :open="open"
    @update:open="(v) => emit('update:open', v)"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent class="dialog-content conn-dialog quick-button-dialog focus:outline-none">
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <Zap
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{
                button
                  ? t("statusBar.quickButtons.editTitle")
                  : t("statusBar.quickButtons.newTitle")
              }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ t("statusBar.quickButtons.description") }}
            </DialogDescription>
          </div>
        </header>

        <form
          class="conn-dialog-body"
          @submit.prevent="submit"
        >
          <label class="conn-field-group">
            <span class="conn-field-label">
              {{ t("statusBar.quickButtons.name") }}
              <span class="scripts-required-mark">*</span>
            </span>
            <input
              v-model="form.name"
              class="ui-input"
              autocomplete="off"
            >
          </label>

          <div class="conn-field-group">
            <span class="conn-field-label">{{ t("statusBar.quickButtons.color") }}</span>
            <div class="quick-color-picker">
              <button
                v-for="color in presetColors"
                :key="color"
                type="button"
                class="quick-color-swatch"
                :class="{ selected: form.color === color }"
                :style="{ backgroundColor: color }"
                :aria-label="color"
                @click="form.color = color"
              />
              <label
                class="quick-color-custom"
                :style="{ backgroundColor: form.color }"
                :title="t('statusBar.quickButtons.customColor')"
              >
                <input
                  v-model="form.color"
                  type="color"
                  class="quick-color-custom-input"
                  :aria-label="t('statusBar.quickButtons.customColor')"
                >
              </label>
            </div>
          </div>

          <label class="conn-field-group">
            <span class="conn-field-label">{{ t("statusBar.quickButtons.behavior") }}</span>
            <UiSelect
              v-model="form.type"
              :options="behaviorOptions"
              @change="form.value = ''"
            />
          </label>

          <label
            v-if="form.type === 'send'"
            class="conn-field-group"
          >
            <span class="conn-field-label">
              {{ t("statusBar.quickButtons.content") }}
              <span class="scripts-required-mark">*</span>
            </span>
            <textarea
              v-model="form.value"
              class="ui-input conn-textarea"
              rows="4"
              :placeholder="t('statusBar.quickButtons.contentPlaceholder')"
            />
          </label>
          <label
            v-else
            class="conn-field-group"
          >
            <span class="conn-field-label">
              {{ t("statusBar.quickButtons.script") }}
              <span class="scripts-required-mark">*</span>
            </span>
            <UiSelect
              v-model="form.value"
              :options="scriptOptions"
              :placeholder="t('statusBar.quickButtons.chooseScript')"
              :disabled="!scriptOptions.length"
            />
            <span
              v-if="!scriptOptions.length"
              class="conn-field-hint"
            >
              {{ t("scripts.picker.empty") }}
            </span>
          </label>
        </form>

        <footer class="conn-dialog-footer">
          <DialogClose as-child>
            <button
              type="button"
              class="ui-button ui-button-secondary"
            >
              {{ t("actions.cancel") }}
            </button>
          </DialogClose>
          <button
            type="button"
            class="ui-button ui-button-primary"
            :disabled="!canSubmit"
            @click="submit"
          >
            {{ t("actions.save") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
.quick-button-dialog {
  width: min(480px, calc(100vw - 32px));
}

.quick-color-picker {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.quick-color-swatch {
  width: 24px;
  height: 24px;
  padding: 0;
  border: 2px solid transparent;
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition:
    border-color var(--motion-duration-base) var(--ease-default),
    box-shadow var(--motion-duration-base) var(--ease-default);
}

.quick-color-swatch:hover,
.quick-color-swatch.selected {
  border-color: var(--bg-primary);
  box-shadow: var(--focus-ring);
}

/* 自定义取色入口沿用 highlight-swatch 的模式：色块 label 包裹透明原生取色器 */
.quick-color-custom {
  position: relative;
  width: 24px;
  height: 24px;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--radius-pill);
  cursor: pointer;
  transition:
    border-color var(--motion-duration-base) var(--ease-default),
    box-shadow var(--motion-duration-base) var(--ease-default);
}

.quick-color-custom:hover,
.quick-color-custom:focus-within {
  border-color: var(--accent);
  box-shadow: var(--focus-ring);
}

.quick-color-custom-input {
  position: absolute;
  inset: 0;
  padding: 0;
  border: none;
  opacity: 0;
  cursor: pointer;
}
</style>
