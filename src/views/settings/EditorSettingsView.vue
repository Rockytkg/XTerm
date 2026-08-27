<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Code2 } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import FontPicker from "../../components/FontPicker.vue";
import UiSelect from "../../components/UiSelect.vue";
import UiSwitch from "../../components/UiSwitch.vue";
import { useSystemFonts } from "../../composables/useSystemFonts";
import { normalizeEditorThemeMode } from "../../utils/editorTheme";
import { normalizeNumberPreference } from "../../utils/numberPreference";

const { t } = useI18n();
const { preferences } = storeToRefs(useWorkspaceStore());
const { systemFonts } = useSystemFonts(() => preferences.value.editorFontFamily);

const editorThemeModeOptions = computed(() => [
  { label: t("settings.editor.themeModes.follow"), value: "follow" },
  { label: t("settings.theme.light"), value: "light" },
  { label: t("settings.theme.dark"), value: "dark" },
]);

function normalizeEditorThemePreference() {
  preferences.value.editorThemeMode = normalizeEditorThemeMode(preferences.value.editorThemeMode);
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <Code2
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.editor.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.editor.hint") }}
        </p>
      </div>
    </div>

    <div class="settings-fields">
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.editor.themeMode") }}</span>
        <UiSelect
          v-model="preferences.editorThemeMode"
          :options="editorThemeModeOptions"
          @change="normalizeEditorThemePreference"
        />
      </div>

      <div class="settings-field">
        <span class="settings-label">{{ t("settings.editor.fontFamily") }}</span>
        <FontPicker
          v-model="preferences.editorFontFamily"
          :fonts="systemFonts"
          :placeholder="t('settings.editor.fontFamilyPlaceholder')"
          :no-results-text="t('settings.editor.noFontsFound')"
        />
      </div>

      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.editor.fontSize") }}</span>
        <input
          v-model.number="preferences.editorFontSize"
          type="number"
          min="10"
          max="28"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'editorFontSize', 14, 10, 28, true)"
        >
      </div>

      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.editor.tabSize") }}</span>
        <input
          v-model.number="preferences.editorTabSize"
          type="number"
          min="1"
          max="8"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'editorTabSize', 2, 1, 8, true)"
        >
      </div>

      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.editor.lineWrapping") }}</span>
          <span class="settings-hint">{{ t("settings.editor.lineWrappingHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.editorLineWrapping" />
      </div>

      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.editor.highlightActiveLine") }}</span>
          <span class="settings-hint">{{ t("settings.editor.highlightActiveLineHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.editorHighlightActiveLine" />
      </div>
    </div>
  </section>
</template>
