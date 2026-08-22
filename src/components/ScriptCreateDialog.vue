<script setup>
import { reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { CirclePlus } from "@lucide/vue";
import { validateScriptFields } from "../services/scripting/formValidation";
import { useScriptsStore } from "../stores/scriptsStore";

const props = defineProps({
  open: { type: Boolean, default: false },
});

const emit = defineEmits(["update:open", "created"]);

const { t } = useI18n();
const scriptsStore = useScriptsStore();

const form = reactive({ name: "", author: "", homepage: "", description: "" });
const fieldErrors = ref({});
const nameInput = ref(null);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    // 作者信息记住上次填写值，脚本名/备注每次清空。
    form.name = "";
    form.description = "";
    form.author = scriptsStore.authorProfile.author;
    form.homepage = scriptsStore.authorProfile.homepage;
    fieldErrors.value = {};
  },
);

function onOpenChange(open) {
  emit("update:open", open);
}

function clearFieldError(key) {
  if (!fieldErrors.value[key]) return;
  const next = { ...fieldErrors.value };
  delete next[key];
  fieldErrors.value = next;
}

function validate() {
  fieldErrors.value = validateScriptFields(
    [
      { key: "name", required: true },
      { key: "homepage", type: "url" },
    ],
    form,
  );
  return !Object.keys(fieldErrors.value).length;
}

function submit() {
  if (!validate()) return;
  const metadata = {
    name: form.name.trim(),
    author: form.author.trim(),
    homepage: form.homepage.trim(),
    description: form.description.trim(),
  };
  void scriptsStore.saveAuthorProfile(metadata);
  const script = scriptsStore.createScript(metadata);
  emit("update:open", false);
  emit("created", script);
}
</script>

<template>
  <DialogRoot
    :open="open"
    @update:open="onOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent class="dialog-content conn-dialog focus:outline-none">
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <CirclePlus
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ t("scripts.createDialog.title") }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ t("scripts.createDialog.description") }}
            </DialogDescription>
          </div>
        </header>

        <form
          class="conn-dialog-body"
          @submit.prevent="submit"
        >
          <label class="conn-field-group">
            <span class="conn-field-label">
              {{ t("scripts.fields.name") }}
              <span class="scripts-required-mark">*</span>
            </span>
            <input
              ref="nameInput"
              v-model="form.name"
              class="ui-input"
              :placeholder="t('scripts.fields.namePlaceholder')"
              autocomplete="off"
              @input="clearFieldError('name')"
            >
            <span
              v-if="fieldErrors.name"
              class="conn-field-error"
            >{{
              t(`scripts.validation.${fieldErrors.name}`)
            }}</span>
          </label>
          <label class="conn-field-group">
            <span class="conn-field-label">{{ t("scripts.fields.author") }}</span>
            <input
              v-model="form.author"
              class="ui-input"
              :placeholder="t('scripts.fields.authorPlaceholder')"
              autocomplete="off"
            >
          </label>
          <label class="conn-field-group">
            <span class="conn-field-label">{{ t("scripts.fields.homepage") }}</span>
            <input
              v-model="form.homepage"
              class="ui-input"
              :placeholder="t('scripts.fields.homepagePlaceholder')"
              autocomplete="off"
              @input="clearFieldError('homepage')"
            >
            <span
              v-if="fieldErrors.homepage"
              class="conn-field-error"
            >{{
              t(`scripts.validation.${fieldErrors.homepage}`)
            }}</span>
          </label>
          <label class="conn-field-group">
            <span class="conn-field-label">{{ t("scripts.fields.description") }}</span>
            <input
              v-model="form.description"
              class="ui-input"
              :placeholder="t('scripts.fields.descriptionPlaceholder')"
              autocomplete="off"
            >
          </label>
        </form>

        <footer class="conn-dialog-footer">
          <button
            type="button"
            class="ui-button-secondary"
            @click="onOpenChange(false)"
          >
            {{ t("actions.cancel") }}
          </button>
          <button
            type="button"
            class="ui-button-primary"
            @click="submit"
          >
            {{ t("scripts.add") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
.scripts-required-mark {
  color: var(--danger);
  margin-left: 2px;
}
</style>
