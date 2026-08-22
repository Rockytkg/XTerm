<script setup>
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { BrushCleaning, Monitor, Moon, Sun } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { ToggleGroupItem, ToggleGroupRoot } from "reka-ui";
import { useWorkspaceStore } from "../../stores/workspaceStore";
// Per-view import: this component renders independently from other consumers of this stylesheet.
import "../../styles/settings-tab-switcher.scss";
import UiSelect from "../../components/UiSelect.vue";
import UiSwitch from "../../components/UiSwitch.vue";

const { t } = useI18n();
const { preferences } = storeToRefs(useWorkspaceStore());

const themeOptions = computed(() => [
  { label: t("settings.theme.light"), value: "light", icon: Sun },
  { label: t("settings.theme.dark"), value: "dark", icon: Moon },
  { label: t("settings.theme.auto"), value: "auto", icon: Monitor },
]);

const uiThemeOptionsLight = computed(() => [
  { label: t("settings.appearance.uiThemes.default"), value: "default" },
  { label: t("settings.appearance.uiThemes.solarizedLight"), value: "solarized" },
  { label: t("settings.appearance.uiThemes.githubLight"), value: "github" },
]);

const uiThemeOptionsDark = computed(() => [
  { label: t("settings.appearance.uiThemes.default"), value: "default" },
  { label: t("settings.appearance.uiThemes.solarizedDark"), value: "solarized" },
  { label: t("settings.appearance.uiThemes.githubDark"), value: "github" },
]);
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <BrushCleaning
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.appearance.title") }}
        </h3>
      </div>
    </div>
    <div class="settings-fields">
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.appearance.theme") }}</span>
        </div>
        <ToggleGroupRoot
          v-model="preferences.theme"
          type="single"
          class="settings-tab-switcher"
        >
          <ToggleGroupItem
            v-for="o in themeOptions"
            :key="o.value"
            :value="o.value"
            class="settings-tab-option"
          >
            <component
              :is="o.icon"
              class="settings-tab-icon"
              :size="13"
              stroke-width="1.8"
            />
            {{ o.label }}
          </ToggleGroupItem>
        </ToggleGroupRoot>
      </div>
      <div class="settings-field">
        <div>
          <span class="settings-label">{{ t("settings.appearance.uiThemeLight") }}</span>
          <span class="settings-hint">{{ t("settings.appearance.uiThemeHint") }}</span>
        </div>
        <UiSelect
          v-model="preferences.uiThemeLight"
          :options="uiThemeOptionsLight"
        />
      </div>
      <div class="settings-field">
        <div>
          <span class="settings-label">{{ t("settings.appearance.uiThemeDark") }}</span>
          <span class="settings-hint">{{ t("settings.appearance.uiThemeHint") }}</span>
        </div>
        <UiSelect
          v-model="preferences.uiThemeDark"
          :options="uiThemeOptionsDark"
        />
      </div>
      <div class="settings-toggle">
        <div>
          <span class="settings-label">{{ t("settings.appearance.enableAnimations") }}</span>
          <span class="settings-hint">{{ t("settings.appearance.enableAnimationsHint") }}</span>
        </div>
        <UiSwitch v-model="preferences.enableAnimations" />
      </div>
      <div class="settings-toggle">
        <span class="settings-label">{{ t("settings.appearance.uiFontSize") }}</span>
        <input
          v-model.number="preferences.uiFontSize"
          type="number"
          min="12"
          max="18"
          class="ui-input ui-input-inline"
        >
      </div>
    </div>
  </section>
</template>
