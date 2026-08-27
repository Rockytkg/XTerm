<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { TerminalSquare } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import UiSelect from "../../components/UiSelect.vue";
import UiSwitch from "../../components/UiSwitch.vue";
import FontPicker from "../../components/FontPicker.vue";
import { useSystemFonts } from "../../composables/useSystemFonts";
import { normalizeNumberPreference } from "../../utils/numberPreference";
import { TERMINAL_TYPE_OPTIONS } from "../../utils/terminalSessionOptions";
import { TERMINAL_THEME_NAMES } from "../../utils/terminalColors";

const { t } = useI18n();
const { preferences } = storeToRefs(useWorkspaceStore());
const { systemFonts } = useSystemFonts(() => preferences.value.terminalFontFamily);

const terminalThemeOptions = computed(() =>
  TERMINAL_THEME_NAMES.map((value) => ({
    label: t(`settings.terminal.themes.${value}`),
    value,
  })),
);

const terminalCursorStyleOptions = computed(() => [
  { label: t("settings.terminal.cursorStyles.block"), value: "block" },
  { label: t("settings.terminal.cursorStyles.underline"), value: "underline" },
  { label: t("settings.terminal.cursorStyles.bar"), value: "bar" },
]);

const terminalCursorInactiveStyleOptions = computed(() => [
  { label: t("settings.terminal.cursorInactiveStyles.outline"), value: "outline" },
  { label: t("settings.terminal.cursorInactiveStyles.block"), value: "block" },
  { label: t("settings.terminal.cursorInactiveStyles.underline"), value: "underline" },
  { label: t("settings.terminal.cursorInactiveStyles.bar"), value: "bar" },
  { label: t("settings.terminal.cursorInactiveStyles.none"), value: "none" },
]);

function normalizeTerminalScrollback() {
  const scrollback = Math.round(Number(preferences.value.terminalScrollback));
  preferences.value.terminalScrollback = Number.isFinite(scrollback)
    ? Math.min(100000, Math.max(100, scrollback))
    : 9001;
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <TerminalSquare
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.terminal.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.terminal.hint") }}
        </p>
      </div>
    </div>
    <div class="settings-fields">
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.themeFollowApp") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.themeFollowAppHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.terminalThemeFollowApp" />
      </div>
      <template v-if="preferences.terminalThemeFollowApp">
        <div class="settings-field">
          <span class="settings-label">{{ t("settings.terminal.themeLightMode") }}</span>
          <UiSelect
            v-model="preferences.terminalThemeLight"
            :options="terminalThemeOptions"
          />
        </div>
        <div class="settings-field">
          <span class="settings-label">{{ t("settings.terminal.themeDarkMode") }}</span>
          <UiSelect
            v-model="preferences.terminalThemeDark"
            :options="terminalThemeOptions"
          />
        </div>
      </template>
      <div
        v-else
        class="settings-field"
      >
        <span class="settings-label">{{ t("settings.terminal.theme") }}</span>
        <UiSelect
          v-model="preferences.terminalTheme"
          :options="terminalThemeOptions"
        />
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.terminal.terminalType") }}</span>
        <UiSelect
          v-model="preferences.terminalType"
          :options="TERMINAL_TYPE_OPTIONS"
        />
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.terminal.fontFamily") }}</span>
        <FontPicker
          v-model="preferences.terminalFontFamily"
          :fonts="systemFonts"
          :placeholder="t('settings.terminal.fontFamilyPlaceholder')"
          :no-results-text="t('settings.terminal.noFontsFound')"
        />
      </div>
      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.terminal.fontSize") }}</span>
        <input
          v-model.number="preferences.terminalFontSize"
          type="number"
          min="8"
          max="36"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalFontSize', 16, 8, 36, true)"
        >
      </div>
      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.terminal.lineHeight") }}</span>
        <input
          v-model.number="preferences.terminalLineHeight"
          type="number"
          min="1"
          max="2"
          step="0.1"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalLineHeight', 1, 1, 2)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.scrollback") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.scrollbackHint") }}</span>
        </div>
        <input
          v-model.number="preferences.terminalScrollback"
          type="number"
          min="100"
          max="100000"
          step="100"
          class="ui-input ui-input-inline"
          @change="normalizeTerminalScrollback"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.cursorBlink") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.cursorBlinkHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.terminalCursorBlink" />
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.terminal.cursorStyle") }}</span>
        <UiSelect
          v-model="preferences.terminalCursorStyle"
          :options="terminalCursorStyleOptions"
        />
      </div>
      <div class="settings-field">
        <span class="settings-label">{{ t("settings.terminal.cursorInactiveStyle") }}</span>
        <UiSelect
          v-model="preferences.terminalCursorInactiveStyle"
          :options="terminalCursorInactiveStyleOptions"
        />
      </div>
      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.terminal.cursorWidth") }}</span>
        <input
          v-model.number="preferences.terminalCursorWidth"
          type="number"
          min="1"
          max="10"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalCursorWidth', 1, 1, 10, true)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.scrollSensitivity") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.scrollSensitivityHint") }}</span>
        </div>
        <input
          v-model.number="preferences.terminalScrollSensitivity"
          type="number"
          min="0.1"
          max="10"
          step="0.1"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalScrollSensitivity', 1, 0.1, 10)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.fastScrollSensitivity") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.fastScrollSensitivityHint") }}</span>
        </div>
        <input
          v-model.number="preferences.terminalFastScrollSensitivity"
          type="number"
          min="1"
          max="20"
          step="0.5"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalFastScrollSensitivity', 5, 1, 20)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.smoothScrollDuration") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.smoothScrollDurationHint") }}</span>
        </div>
        <input
          v-model.number="preferences.terminalSmoothScrollDuration"
          type="number"
          min="0"
          max="1000"
          step="50"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalSmoothScrollDuration', 0, 0, 1000, true)"
        >
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.terminal.minimumContrastRatio") }}</span>
          <span class="settings-hint">{{ t("settings.terminal.minimumContrastRatioHint") }}</span>
        </div>
        <input
          v-model.number="preferences.terminalMinimumContrastRatio"
          type="number"
          min="1"
          max="21"
          step="0.5"
          class="ui-input ui-input-inline"
          @change="normalizeNumberPreference(preferences, 'terminalMinimumContrastRatio', 1, 1, 21)"
        >
      </div>
      <div
        v-for="item in [
          ['terminalAltClickMovesCursor', 'altClickMovesCursor', 'altClickMovesCursorHint'],
          ['terminalRightClickSelectsWord', 'rightClickSelectsWord', 'rightClickSelectsWordHint'],
          ['terminalScrollOnUserInput', 'scrollOnUserInput', 'scrollOnUserInputHint'],
          [
            'terminalScrollOnEraseInDisplay',
            'scrollOnEraseInDisplay',
            'scrollOnEraseInDisplayHint',
          ],
          [
            'terminalDrawBoldTextInBrightColors',
            'drawBoldTextInBrightColors',
            'drawBoldTextInBrightColorsHint',
          ],
          ['terminalCustomGlyphs', 'customGlyphs', 'customGlyphsHint'],
          [
            'terminalRescaleOverlappingGlyphs',
            'rescaleOverlappingGlyphs',
            'rescaleOverlappingGlyphsHint',
          ],
          ['terminalMacOptionIsMeta', 'macOptionIsMeta', 'macOptionIsMetaHint'],
          [
            'terminalMacOptionClickForcesSelection',
            'macOptionClickForcesSelection',
            'macOptionClickForcesSelectionHint',
          ],
          ['terminalWebgl', 'webgl', 'webglHint'],
        ]"
        :key="item[0]"
        class="settings-toggle"
      >
        <div>
          <span class="settings-label">{{ t(`settings.terminal.${item[1]}`) }}</span>
          <span class="settings-hint">{{ t(`settings.terminal.${item[2]}`) }}</span>
        </div>
        <UiSwitch v-model="preferences[item[0]]" />
      </div>
    </div>
  </section>
</template>
