<script setup>
import { computed } from "vue";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { Copy as RestoreIcon, Minus, Monitor, Moon, Square, Sun, X } from "@lucide/vue";
import AppTooltip from "../../components/AppTooltip.vue";
import { useTauriWindowController } from "../../composables/useTauriWindowController";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const { t } = useI18n();
const workspace = useWorkspaceStore();
const { preferences } = storeToRefs(workspace);
const { closeWindow, isWindowMaximized, minimizeWindow, toggleWindowMaximize } =
  useTauriWindowController();

const titlebarThemeIcon = computed(() => {
  if (preferences.value.theme === "light") return Moon;
  if (preferences.value.theme === "dark") return Monitor;
  return Sun;
});

function releaseTitlebarButtonFocus(event) {
  event?.currentTarget?.blur?.();
}

function toggleTitlebarTheme(event) {
  workspace.toggleTheme(event);
  releaseTitlebarButtonFocus(event);
}

function titlebarAction(fn) {
  return (event) => {
    releaseTitlebarButtonFocus(event);
    fn();
  };
}

const minimizeTitlebarWindow = titlebarAction(minimizeWindow);
const toggleTitlebarWindowMaximize = titlebarAction(toggleWindowMaximize);
const closeTitlebarWindow = titlebarAction(closeWindow);
</script>

<template>
  <header
    class="shell-titlebar"
    data-tauri-drag-region="deep"
  >
    <div
      class="titlebar-brand"
      data-tauri-drag-region
    >
      <svg
        class="titlebar-logo"
        width="18"
        height="18"
        viewBox="0 0 64 64"
        fill="none"
      >
        <rect
          width="64"
          height="64"
          rx="14"
          fill="oklch(0 0 0)"
        />
        <path
          d="M16 20l10 10-10 10"
          stroke="oklch(1 0 0)"
          stroke-width="5"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <path
          d="M31 42h17"
          stroke="oklch(0.63 0.21 255)"
          stroke-width="5"
          stroke-linecap="round"
        />
      </svg>
      <span class="titlebar-appname">XTerm</span>
    </div>

    <div
      class="titlebar-right"
      data-tauri-drag-region="deep"
    >
      <AppTooltip
        :content="
          preferences.theme === 'light'
            ? t('header.darkMode')
            : preferences.theme === 'dark'
              ? t('header.autoMode')
              : t('header.lightMode')
        "
        side="bottom"
      >
        <button
          type="button"
          class="titlebar-tool-button"
          @click="toggleTitlebarTheme"
        >
          <component
            :is="titlebarThemeIcon"
            class="theme-icon"
            :size="14"
            stroke-width="1.8"
          />
        </button>
      </AppTooltip>
      <div
        class="titlebar-window-controls"
        aria-label="Window controls"
      >
        <AppTooltip
          :content="t('header.minimize')"
          side="bottom"
        >
          <button
            type="button"
            class="titlebar-window-control"
            :aria-label="t('header.minimize')"
            @click="minimizeTitlebarWindow"
          >
            <Minus
              :size="14"
              stroke-width="1.9"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="isWindowMaximized ? t('header.restore') : t('header.maximize')"
          side="bottom"
        >
          <button
            type="button"
            class="titlebar-window-control"
            :aria-label="isWindowMaximized ? t('header.restore') : t('header.maximize')"
            @click="toggleTitlebarWindowMaximize"
          >
            <RestoreIcon
              v-if="isWindowMaximized"
              :size="13"
              stroke-width="1.8"
            />
            <Square
              v-else
              :size="12"
              stroke-width="1.9"
            />
          </button>
        </AppTooltip>
        <AppTooltip
          :content="t('header.closeWindow')"
          side="bottom"
        >
          <button
            type="button"
            class="titlebar-window-control titlebar-window-close"
            :aria-label="t('header.closeWindow')"
            @click="closeTitlebarWindow"
          >
            <X
              :size="14"
              stroke-width="1.9"
            />
          </button>
        </AppTooltip>
      </div>
    </div>
  </header>
</template>
