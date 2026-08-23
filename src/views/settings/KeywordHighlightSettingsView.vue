<script setup>
import { computed, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  CirclePlus,
  Download,
  Highlighter,
  Palette,
  Pencil,
  Plus,
  SquarePen,
  Trash2,
  Upload,
  X,
} from "@lucide/vue";
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import AppTooltip from "../../components/AppTooltip.vue";
import ConfirmDialog from "../../components/ConfirmDialog.vue";
import UiMultiSelect from "../../components/UiMultiSelect.vue";
import KeywordHighlightSchemeEditor from "./KeywordHighlightSchemeEditor.vue";
import { useHighlightSchemes } from "../../composables/useHighlightSchemes";

const { t } = useI18n();
const {
  schemes,
  terminalThemeOptions,
  addScheme,
  importSchemes,
  exportScheme,
  removeScheme,
  updateScheme,
  setSchemeThemes,
} = useHighlightSchemes();

const editingSchemeId = ref("");
const editingScheme = computed(
  () => schemes.value.find((scheme) => scheme.id === editingSchemeId.value) || null,
);

const schemeDialog = ref(null);
const schemeFormName = ref("");
const schemeFormThemes = ref([]);
const schemeNameInput = ref(null);

// 关闭时 schemeDialog 立即置空，而退出动画仍在播放；锁存 mode 让标题、
// 图标与按钮文案在动画期间保持关闭前的取值，不闪回“新建”。
const schemeDialogMode = ref("create");
watch(schemeDialog, (dialog) => {
  if (dialog) schemeDialogMode.value = dialog.mode;
});

const deleteTarget = ref(null);
const deleteDialogOpen = computed({
  get: () => !!deleteTarget.value,
  set: (value) => {
    if (!value) deleteTarget.value = null;
  },
});

function openCreateSchemeDialog() {
  schemeFormName.value = "";
  schemeFormThemes.value = [];
  schemeDialog.value = { mode: "create" };
}

function openEditSchemeDialog(scheme) {
  schemeFormName.value = scheme.name || "";
  schemeFormThemes.value = Array.isArray(scheme.themes) ? [...scheme.themes] : [];
  schemeDialog.value = { mode: "edit", id: scheme.id };
}

function closeSchemeDialog(open) {
  if (!open) schemeDialog.value = null;
}

function focusSchemeNameInput(event) {
  event.preventDefault();
  nextTick(() => schemeNameInput.value?.focus());
}

function confirmSchemeDialog() {
  const dialog = schemeDialog.value;
  if (!dialog) return;
  const name = schemeFormName.value.trim();
  const themes = [...schemeFormThemes.value];
  if (dialog.mode === "create") {
    editingSchemeId.value = addScheme({ name, themes });
  } else {
    updateScheme(dialog.id, { name });
    setSchemeThemes(dialog.id, themes);
  }
  schemeDialog.value = null;
}

function confirmRemoveScheme() {
  if (deleteTarget.value) removeScheme(deleteTarget.value.id);
  deleteTarget.value = null;
}

function getSchemeMeta(scheme) {
  return t("settings.terminal.highlightSchemeMeta", {
    rules: scheme.rules?.length || 0,
    themes: scheme.themes?.length || 0,
  });
}
</script>

<template>
  <section class="settings-section highlight-settings-section">
    <KeywordHighlightSchemeEditor
      v-if="editingScheme"
      :scheme="editingScheme"
      @back="editingSchemeId = ''"
    />

    <template v-else>
      <div class="settings-section-header">
        <Palette
          :size="16"
          stroke-width="1.8"
          class="text-accent"
        />
        <div>
          <h3 class="settings-section-title">
            {{ t("settings.terminal.highlightTitle") }}
          </h3>
          <p class="settings-section-desc">
            {{ t("settings.terminal.highlightHint") }}
          </p>
        </div>
        <div class="highlight-header-actions">
          <button
            type="button"
            class="ui-button-secondary highlight-action"
            @click="importSchemes"
          >
            <Download
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.terminal.highlightImportSchemes") }}
          </button>
          <button
            type="button"
            class="ui-button-secondary highlight-action"
            @click="openCreateSchemeDialog"
          >
            <Plus
              :size="13"
              stroke-width="1.8"
            />
            {{ t("settings.terminal.highlightAddScheme") }}
          </button>
        </div>
      </div>

      <div
        v-if="schemes.length"
        class="highlight-scheme-list"
      >
        <div
          v-for="scheme in schemes"
          :key="scheme.id"
          class="highlight-scheme-card"
          @click="editingSchemeId = scheme.id"
        >
          <span class="highlight-scheme-tile">
            <Highlighter
              :size="15"
              stroke-width="1.8"
            />
          </span>
          <div class="highlight-scheme-main">
            <span class="highlight-scheme-name">
              {{ scheme.name || t("settings.terminal.highlightUntitled") }}
            </span>
            <span class="highlight-scheme-meta">{{ getSchemeMeta(scheme) }}</span>
          </div>
          <div
            class="highlight-scheme-actions"
            @click.stop
          >
            <AppTooltip
              :content="t('settings.terminal.highlightSchemeDialogEditTitle')"
              side="top"
            >
              <button
                type="button"
                class="highlight-icon-button"
                :aria-label="t('settings.terminal.highlightSchemeDialogEditTitle')"
                @click="openEditSchemeDialog(scheme)"
              >
                <Pencil
                  :size="13"
                  stroke-width="1.8"
                />
              </button>
            </AppTooltip>
            <AppTooltip
              :content="t('settings.terminal.highlightExportSchemes')"
              side="top"
            >
              <button
                type="button"
                class="highlight-icon-button"
                :aria-label="t('settings.terminal.highlightExportSchemes')"
                @click="exportScheme(scheme.id)"
              >
                <Upload
                  :size="13"
                  stroke-width="1.8"
                />
              </button>
            </AppTooltip>
            <AppTooltip
              :content="t('settings.terminal.highlightRemoveScheme')"
              side="top"
            >
              <button
                type="button"
                class="highlight-icon-button highlight-icon-button-danger"
                :aria-label="t('settings.terminal.highlightRemoveScheme')"
                @click="deleteTarget = scheme"
              >
                <Trash2
                  :size="13"
                  stroke-width="1.8"
                />
              </button>
            </AppTooltip>
          </div>
        </div>
      </div>

      <div
        v-else
        class="highlight-empty"
      >
        <Palette
          :size="28"
          stroke-width="1.5"
          class="text-tertiary"
        />
        <span class="settings-hint">{{ t("settings.terminal.highlightEmpty") }}</span>
        <button
          type="button"
          class="ui-button-secondary highlight-action"
          @click="openCreateSchemeDialog"
        >
          <Plus
            :size="13"
            stroke-width="1.8"
          />
          {{ t("settings.terminal.highlightAddScheme") }}
        </button>
      </div>
    </template>

    <DialogRoot
      :open="!!schemeDialog"
      @update:open="closeSchemeDialog"
    >
      <DialogPortal>
        <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
        <DialogContent
          class="dialog-content conn-dialog highlight-scheme-dialog focus:outline-none"
          @open-auto-focus="focusSchemeNameInput"
        >
          <header class="conn-dialog-header">
            <div
              class="conn-dialog-header-icon"
              aria-hidden="true"
            >
              <component
                :is="schemeDialogMode === 'edit' ? SquarePen : CirclePlus"
                :size="16"
                stroke-width="1.8"
              />
            </div>
            <div class="flex-1 min-w-0">
              <DialogTitle class="conn-dialog-title">
                {{
                  t(
                    schemeDialogMode === "edit"
                      ? "settings.terminal.highlightSchemeDialogEditTitle"
                      : "settings.terminal.highlightSchemeDialogCreateTitle",
                  )
                }}
              </DialogTitle>
              <DialogDescription class="conn-dialog-desc">
                {{ t("settings.terminal.highlightSchemeDialogDescription") }}
              </DialogDescription>
            </div>
            <DialogClose as-child>
              <button
                type="button"
                class="ui-icon-button shrink-0"
                :aria-label="t('actions.closeDialog')"
              >
                <X
                  :size="15"
                  stroke-width="1.8"
                />
              </button>
            </DialogClose>
          </header>

          <form
            class="conn-dialog-body"
            @submit.prevent="confirmSchemeDialog"
          >
            <label class="conn-field-group">
              <span class="conn-field-label">{{ t("settings.terminal.highlightSchemeName") }}</span>
              <input
                ref="schemeNameInput"
                v-model="schemeFormName"
                class="ui-input"
                :placeholder="t('settings.terminal.highlightNewScheme')"
              >
            </label>
            <div class="conn-field-group">
              <span class="conn-field-label">
                {{ t("settings.terminal.highlightThemeBindings") }}
              </span>
              <UiMultiSelect
                v-model="schemeFormThemes"
                :options="terminalThemeOptions"
                :placeholder="t('settings.terminal.highlightThemePickerPlaceholder')"
                :search-placeholder="t('settings.terminal.highlightThemeSearchPlaceholder')"
                :empty-text="t('settings.terminal.highlightThemeSearchEmpty')"
              />
            </div>
          </form>

          <footer class="conn-dialog-footer">
            <DialogClose as-child>
              <button
                type="button"
                class="ui-button-secondary"
              >
                {{ t("actions.cancel") }}
              </button>
            </DialogClose>
            <button
              type="button"
              class="ui-button-primary"
              @click="confirmSchemeDialog"
            >
              {{
                schemeDialogMode === "edit"
                  ? t("actions.save")
                  : t("settings.terminal.highlightAddScheme")
              }}
            </button>
          </footer>
        </DialogContent>
      </DialogPortal>
    </DialogRoot>

    <ConfirmDialog
      v-model:open="deleteDialogOpen"
      tone="danger"
      :title="t('settings.terminal.highlightRemoveConfirmTitle')"
      :description="
        t('settings.terminal.highlightRemoveConfirmDescription', {
          name: deleteTarget?.name || t('settings.terminal.highlightUntitled'),
        })
      "
      :confirm-text="t('actions.delete')"
      :confirm-icon="Trash2"
      @confirm="confirmRemoveScheme"
    />
  </section>
</template>
