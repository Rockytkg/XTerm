<script setup>
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  BrushCleaning,
  Code2,
  FolderCog,
  Info,
  Keyboard,
  Languages,
  Palette,
  Power,
  SlidersHorizontal,
  TerminalSquare,
  UploadCloud,
} from "@lucide/vue";
import { TabsList, TabsRoot, TabsTrigger } from "reka-ui";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useToasts } from "../composables/useToasts";
import { restartApp } from "../services/appInfo";
import { createPanelTransitionHooks, motionEnabled } from "../utils/motion";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import GeneralSettingsView from "../views/settings/GeneralSettingsView.vue";
import PathsSettingsView from "../views/settings/PathsSettingsView.vue";
import AppearanceSettingsView from "../views/settings/AppearanceSettingsView.vue";
import EditorSettingsView from "../views/settings/EditorSettingsView.vue";
import TerminalSettingsView from "../views/settings/TerminalSettingsView.vue";
import TransferSettingsView from "../views/settings/TransferSettingsView.vue";
import KeywordHighlightSettingsView from "../views/settings/KeywordHighlightSettingsView.vue";
import ShortcutsSettingsView from "../views/settings/ShortcutsSettingsView.vue";
import AboutSettingsView from "../views/settings/AboutSettingsView.vue";
import "../styles/settings-layout.scss";
import "../styles/settings-fields.scss";

const { t } = useI18n();
const workspace = useWorkspaceStore();
const { preferences, resetPreferences } = workspace;
const { showToast } = useToasts();

const activeSection = ref("general");
const resetConfirmOpen = ref(false);
const restartConfirmOpen = ref(false);

const sections = [
  { id: "general", icon: Languages },
  { id: "paths", icon: FolderCog },
  { id: "appearance", icon: BrushCleaning },
  { id: "terminal", icon: TerminalSquare },
  { id: "editor", icon: Code2 },
  { id: "transfer", icon: UploadCloud },
  { id: "highlight", icon: Palette },
  { id: "shortcuts", icon: Keyboard },
  { id: "about", icon: Info },
];

const sectionComponents = {
  general: GeneralSettingsView,
  paths: PathsSettingsView,
  appearance: AppearanceSettingsView,
  terminal: TerminalSettingsView,
  editor: EditorSettingsView,
  transfer: TransferSettingsView,
  highlight: KeywordHighlightSettingsView,
  shortcuts: ShortcutsSettingsView,
  about: AboutSettingsView,
};

const activeSectionComponent = computed(
  () => sectionComponents[activeSection.value] || GeneralSettingsView,
);
const settingsPanelMotionEnabled = computed(
  () => preferences.enableAnimations !== false && motionEnabled(),
);
const settingsPanelTransition = createPanelTransitionHooks();

function setActiveSection(section) {
  if (!sectionComponents[section] || section === activeSection.value) return;
  activeSection.value = section;
}

async function confirmResetPreferences() {
  try {
    await resetPreferences();
    resetConfirmOpen.value = false;
    restartConfirmOpen.value = true;
    showToast({ type: "success", title: t("notifications.preferencesReset") });
  } catch (error) {
    showToast({
      type: "error",
      title: t("notifications.preferencesResetFailed"),
      message: String(error),
    });
  }
}

async function confirmRestartApp() {
  try {
    await restartApp();
  } catch (error) {
    showToast({
      type: "error",
      title: t("notifications.appRestartFailed"),
      message: String(error),
    });
  }
}
</script>

<template>
  <TabsRoot
    :model-value="activeSection"
    class="settings-root"
    orientation="vertical"
    activation-mode="manual"
    @update:model-value="setActiveSection"
  >
    <aside class="settings-rail">
      <div class="settings-nav-brand">
        <div class="ui-overline">
          {{ t("settings.surfaceLabel") }}
        </div>
        <h2 class="settings-nav-title">
          {{ t("settings.title") }}
        </h2>
      </div>
      <TabsList class="settings-nav-links">
        <TabsTrigger
          v-for="s in sections"
          :key="s.id"
          :value="s.id"
          class="ui-settings-nav-link"
          :class="activeSection === s.id ? 'ui-settings-nav-link-active' : ''"
        >
          <component
            :is="s.icon"
            :size="15"
            stroke-width="1.8"
          />
          {{ t(`settings.sections.${s.id}`) }}
        </TabsTrigger>
      </TabsList>
      <button
        type="button"
        class="settings-reset-btn"
        @click="resetConfirmOpen = true"
      >
        <SlidersHorizontal
          :size="13"
          stroke-width="1.8"
        />
        {{ t("settings.reset") }}
      </button>
    </aside>

    <div class="settings-detail">
      <Transition
        v-if="settingsPanelMotionEnabled"
        :css="settingsPanelTransition.css"
        mode="out-in"
        @before-enter="settingsPanelTransition.beforeEnter"
        @enter="settingsPanelTransition.enter"
        @leave="settingsPanelTransition.leave"
      >
        <KeepAlive>
          <component
            :is="activeSectionComponent"
            :key="activeSection"
            class="settings-detail-panel"
          />
        </KeepAlive>
      </Transition>
      <KeepAlive v-else>
        <component
          :is="activeSectionComponent"
          :key="activeSection"
          class="settings-detail-panel"
        />
      </KeepAlive>
    </div>

    <ConfirmDialog
      v-model:open="resetConfirmOpen"
      tone="warning"
      :title="t('settings.resetConfirm.title')"
      :description="t('settings.resetConfirm.description')"
      :confirm-text="t('settings.resetConfirm.confirm')"
      :confirm-icon="SlidersHorizontal"
      @confirm="confirmResetPreferences"
    />
    <ConfirmDialog
      v-model:open="restartConfirmOpen"
      tone="info"
      :title="t('settings.paths.restartConfirm.title')"
      :description="t('settings.paths.restartConfirm.resetDescription')"
      :confirm-text="t('settings.paths.restartConfirm.restartNow')"
      :cancel-text="t('settings.paths.restartConfirm.later')"
      :confirm-icon="Power"
      @confirm="confirmRestartApp"
    />
  </TabsRoot>
</template>
