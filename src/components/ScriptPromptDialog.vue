<script setup>
import { computed, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { FileCode } from "@lucide/vue";
import UiSelect from "./UiSelect.vue";
import UiSwitch from "./UiSwitch.vue";
import { validateScriptFields } from "../services/scripting/formValidation";
import { resolveScriptPrompt, scriptPrompt } from "../services/scripting/scriptPromptController";
import { sanitizeHtml } from "../utils/sanitizeHtml";

const { t } = useI18n();
const inputValue = ref("");
const inputRef = ref(null);
const formValues = ref({});
const fieldErrors = ref({});

const prompt = computed(() => scriptPrompt.value);
const promptTitle = computed(
  () => prompt.value?.title || prompt.value?.scriptName || t("scripts.prompt.defaultTitle"),
);
const formFields = computed(() => {
  if (prompt.value?.kind !== "form" || !Array.isArray(prompt.value.fields)) return [];
  return prompt.value.fields.filter((field) => field?.key);
});
const isFormLike = computed(() => ["input", "form"].includes(prompt.value?.kind));
// html:true 时 message 按 HTML 渲染，渲染前经白名单消毒（sanitizeHtml）防 XSS。
const isHtmlMessage = computed(() => prompt.value?.html === true && !!prompt.value?.message);
const sanitizedMessage = computed(() =>
  isHtmlMessage.value ? sanitizeHtml(prompt.value.message) : "",
);

function defaultFieldValue(field) {
  if (field.defaultValue !== undefined) return field.defaultValue;
  if (["switch", "checkbox"].includes(field.type)) return false;
  return "";
}

watch(prompt, async (next) => {
  fieldErrors.value = {};
  if (next?.kind === "input") {
    inputValue.value = next.defaultValue ?? "";
    await nextTick();
    inputRef.value?.focus();
    inputRef.value?.select();
    return;
  }
  if (next?.kind === "form") {
    const values = {};
    for (const field of formFields.value) values[field.key] = defaultFieldValue(field);
    formValues.value = values;
  }
});

function cancelValue() {
  // confirm 的取消是否定回答；alert 关闭等同确认；input/form 取消 = 取消脚本执行。
  if (prompt.value?.kind === "confirm") return false;
  if (prompt.value?.kind === "alert") return true;
  return null;
}

function resolveCurrentPrompt(value) {
  resolveScriptPrompt(value, prompt.value?.requestId);
}

function onOpenChange(open) {
  if (!open) resolveCurrentPrompt(cancelValue());
}

function clearFieldError(key) {
  if (!fieldErrors.value[key]) return;
  const next = { ...fieldErrors.value };
  delete next[key];
  fieldErrors.value = next;
}

function fieldErrorLabel(key) {
  const code = fieldErrors.value[key];
  if (!code) return "";
  // 字段可用 message（form）/ errorMessage（input）自定义错误文案。
  if (prompt.value?.kind === "form") {
    const field = formFields.value.find((item) => item.key === key);
    if (field?.message) return field.message;
  }
  if (prompt.value?.kind === "input" && prompt.value?.errorMessage) {
    return prompt.value.errorMessage;
  }
  return t(`scripts.validation.${code}`);
}

function validateInput() {
  const errors = validateScriptFields(
    [
      {
        key: "value",
        required: prompt.value?.required,
        type: prompt.value?.type,
        pattern: prompt.value?.pattern,
      },
    ],
    { value: inputValue.value },
  );
  fieldErrors.value = errors.value ? { value: errors.value } : {};
  return !fieldErrors.value.value;
}

function validateForm() {
  fieldErrors.value = validateScriptFields(formFields.value, formValues.value);
  return !Object.keys(fieldErrors.value).length;
}

function submit() {
  const kind = prompt.value?.kind;
  if (kind === "input") {
    if (!validateInput()) {
      nextTick(() => inputRef.value?.focus());
      return;
    }
    resolveCurrentPrompt(inputValue.value);
    return;
  }
  if (kind === "form") {
    if (!validateForm()) return;
    resolveCurrentPrompt({ ...formValues.value });
    return;
  }
  // confirm / alert 都只有一个肯定结果。
  resolveCurrentPrompt(true);
}
</script>

<template>
  <DialogRoot
    v-if="prompt"
    :open="true"
    @update:open="onOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent class="dialog-content conn-dialog script-prompt-dialog focus:outline-none">
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <FileCode
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ promptTitle }}
            </DialogTitle>
            <DialogDescription
              v-if="isHtmlMessage"
              as="div"
              class="conn-dialog-desc script-prompt-message script-prompt-html"
            >
              <!-- 内容已经过 sanitizeHtml 白名单消毒，可安全渲染 -->
              <!-- eslint-disable-next-line vue/no-v-html -->
              <div v-html="sanitizedMessage" />
            </DialogDescription>
            <DialogDescription
              v-else-if="prompt.message"
              class="conn-dialog-desc script-prompt-message"
            >
              {{ prompt.message }}
            </DialogDescription>
          </div>
        </header>

        <form
          v-if="isFormLike"
          class="conn-dialog-body"
          @submit.prevent="submit"
        >
          <label
            v-if="prompt.kind === 'input'"
            class="conn-field-group"
          >
            <input
              ref="inputRef"
              v-model="inputValue"
              class="ui-input"
              :type="prompt.password ? 'password' : 'text'"
              :placeholder="prompt.placeholder || ''"
              autocomplete="off"
              @input="clearFieldError('value')"
            >
            <span
              v-if="fieldErrors.value"
              class="conn-field-error"
            >{{
              fieldErrorLabel("value")
            }}</span>
          </label>

          <template v-else>
            <label
              v-for="field in formFields"
              :key="field.key"
              class="conn-field-group"
            >
              <span class="conn-field-label">
                {{ field.label || field.key }}
                <span
                  v-if="field.required"
                  class="scripts-required-mark"
                >*</span>
              </span>
              <UiSwitch
                v-if="['switch', 'checkbox'].includes(field.type)"
                v-model="formValues[field.key]"
              />
              <UiSelect
                v-else-if="field.type === 'select'"
                v-model="formValues[field.key]"
                :options="field.options || []"
                :placeholder="field.placeholder || ''"
              />
              <input
                v-else
                v-model="formValues[field.key]"
                class="ui-input"
                :type="field.type === 'password' ? 'password' : 'text'"
                :placeholder="field.placeholder || ''"
                autocomplete="off"
                @input="clearFieldError(field.key)"
              >
              <span
                v-if="fieldErrors[field.key]"
                class="conn-field-error"
              >{{
                fieldErrorLabel(field.key)
              }}</span>
            </label>
          </template>
        </form>

        <footer class="conn-dialog-footer">
          <button
            v-if="prompt.kind !== 'alert'"
            type="button"
            class="ui-button-secondary"
            @click="resolveCurrentPrompt(cancelValue())"
          >
            {{ t("actions.cancel") }}
          </button>
          <button
            type="button"
            class="ui-button-primary"
            @click="submit"
          >
            {{ t("actions.confirm") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
/* 纯文本消息按 \n 换行渲染 */
.script-prompt-message {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

/* v-html 渲染的内容不受 scoped 属性选择器覆盖，需要 :deep() */
.script-prompt-html {
  :deep(a) {
    color: var(--accent);
    text-decoration: underline;
  }

  :deep(pre) {
    margin: 6px 0;
    padding: 6px 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  :deep(code) {
    font-family: var(--font-mono);
    font-size: 0.92em;
  }

  :deep(p),
  :deep(ul),
  :deep(ol),
  :deep(blockquote),
  :deep(table) {
    margin: 4px 0;
  }

  :deep(ul),
  :deep(ol) {
    padding-left: 1.4em;
  }

  :deep(blockquote) {
    padding-left: 8px;
    border-left: 3px solid var(--border);
    color: var(--text-secondary);
  }

  :deep(th),
  :deep(td) {
    padding: 2px 8px;
    border: 1px solid var(--border);
  }
}
</style>
