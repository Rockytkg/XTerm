<script setup>
import { computed, nextTick } from "vue";
import { storeToRefs } from "pinia";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { FileCode, KeyRound, Network, Settings2 } from "@lucide/vue";
import AppTooltip from "../../components/AppTooltip.vue";
import { useWorkspaceStore } from "../../stores/workspaceStore";
import { useWorkspaceUiStore } from "../../stores/workspaceUiStore";
import { runViewTransition } from "../../utils/motion";

defineProps({
  navItems: { type: Array, default: () => [] },
});

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const workspace = useWorkspaceStore();
const workspaceUi = useWorkspaceUiStore();
const { activeTab } = storeToRefs(workspace);
const { navExpanded } = storeToRefs(workspaceUi);

let workspaceTabTransitionActive = false;

const isSettings = computed(() => route.name === "settings");
const isSessions = computed(() => route.name === "sessions");
const isWorkspace = computed(() => route.name === "workspace");
const isKeys = computed(() => route.name === "keys");
const isScripts = computed(() => route.name === "scripts");
const navGroupPaddingClass = computed(() => (navExpanded.value ? "px-[6px]" : "pl-[6px] pr-[5px]"));
const collapsedNavItemClass = computed(() =>
  navExpanded.value ? "" : "w-[36px] justify-center px-0!",
);

function navigate(name) {
  router.push({ name });
}

async function runWorkspaceTabTransition(tabId) {
  if (workspaceTabTransitionActive) {
    workspace.selectTab(tabId);
    return;
  }

  workspaceTabTransitionActive = true;

  try {
    await runViewTransition(async () => {
      workspace.selectTab(tabId);
      await nextTick();
    });
  } finally {
    workspaceTabTransitionActive = false;
  }
}

function openWorkspaceTab(tabId) {
  if (activeTab.value === tabId) {
    if (!isWorkspace.value) navigate("workspace");
    return;
  }

  if (isWorkspace.value) {
    void runWorkspaceTabTransition(tabId).catch(() => {});
    return;
  }

  workspace.selectTab(tabId);
  navigate("workspace");
}
</script>

<template>
  <nav
    class="flex flex-col justify-between border-r border-border bg-bg-secondary px-0 py-[8px] overflow-hidden transition-[width,min-width] duration-[var(--motion-duration-base)] ease-[var(--ease-default)] will-change-[width]"
    :class="navExpanded ? 'w-[180px] min-w-[180px]' : 'w-[48px] min-w-[48px]'"
  >
    <div
      class="flex flex-col items-stretch gap-[2px]"
      :class="navGroupPaddingClass"
    >
      <AppTooltip
        :content="navExpanded ? t('nav.collapse') : t('nav.expand')"
        side="right"
      >
        <button
          type="button"
          class="ui-nav-item h-[34px]!"
          :class="collapsedNavItemClass"
          @click="workspaceUi.toggleNavExpanded"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
          >
            <rect
              x="1"
              y="2"
              width="14"
              height="12"
              rx="2.5"
              stroke="currentColor"
              stroke-width="1.4"
            />
            <path
              d="M5.5 2v12"
              stroke="currentColor"
              stroke-width="1.4"
            />
            <path
              v-if="!navExpanded"
              d="M8.5 6l2.5 2-2.5 2"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
            <path
              v-else
              d="M10.5 6L8 8l2.5 2"
              stroke="currentColor"
              stroke-width="1.4"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t("nav.collapseSidebar") }}
          </span>
        </button>
      </AppTooltip>

      <div class="h-[1px] bg-border-light mx-[6px] my-[4px]" />

      <AppTooltip
        :content="t('settings.sections.sessions')"
        side="right"
        :disabled="navExpanded"
      >
        <button
          type="button"
          class="ui-nav-item"
          :class="[isSessions ? 'ui-nav-item-active' : '', collapsedNavItemClass]"
          @click="navigate('sessions')"
        >
          <Network
            :size="18"
            stroke-width="1.8"
          />
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t("settings.sections.sessions") }}
          </span>
        </button>
      </AppTooltip>

      <div class="h-[1px] bg-border-light mx-[6px] my-[4px]" />

      <AppTooltip
        v-for="item in navItems"
        :key="item.id"
        :content="t(item.labelKey)"
        side="right"
        :disabled="navExpanded"
      >
        <button
          type="button"
          class="ui-nav-item"
          :class="[
            activeTab === item.id && isWorkspace ? 'ui-nav-item-active' : '',
            collapsedNavItemClass,
          ]"
          @click="openWorkspaceTab(item.id)"
        >
          <component
            :is="item.icon"
            :size="18"
            stroke-width="1.8"
          />
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t(item.labelKey) }}
          </span>
        </button>
      </AppTooltip>
    </div>

    <div
      class="flex flex-col items-stretch gap-[2px]"
      :class="navGroupPaddingClass"
    >
      <AppTooltip
        :content="t('nav.scripts')"
        side="right"
        :disabled="navExpanded"
      >
        <button
          type="button"
          class="ui-nav-item"
          :class="[isScripts ? 'ui-nav-item-active' : '', collapsedNavItemClass]"
          @click="navigate('scripts')"
        >
          <FileCode
            :size="18"
            stroke-width="1.8"
          />
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t("nav.scripts") }}
          </span>
        </button>
      </AppTooltip>
      <AppTooltip
        :content="t('nav.keys')"
        side="right"
        :disabled="navExpanded"
      >
        <button
          type="button"
          class="ui-nav-item"
          :class="[isKeys ? 'ui-nav-item-active' : '', collapsedNavItemClass]"
          @click="navigate('keys')"
        >
          <KeyRound
            :size="18"
            stroke-width="1.8"
          />
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t("nav.keys") }}
          </span>
        </button>
      </AppTooltip>
      <AppTooltip
        :content="t('nav.settings')"
        side="right"
        :disabled="navExpanded"
      >
        <button
          type="button"
          class="ui-nav-item"
          :class="[isSettings ? 'ui-nav-item-active' : '', collapsedNavItemClass]"
          @click="navigate('settings')"
        >
          <Settings2
            :size="18"
            stroke-width="1.8"
          />
          <span
            v-if="navExpanded"
            class="overflow-hidden text-ellipsis text-[0.8929em] font-500"
          >
            {{ t("nav.settings") }}
          </span>
        </button>
      </AppTooltip>
    </div>
  </nav>
</template>
