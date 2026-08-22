<script setup>
import { computed, nextTick, ref, watch } from "vue";
import { Plus, X } from "@lucide/vue";
import { TabsList, TabsRoot, TabsTrigger } from "reka-ui";
import ConnectDialog from "./ConnectDialog.vue";
import AppTooltip from "./AppTooltip.vue";
import { useWorkspaceTabDragSort } from "../composables/useWorkspaceTabDragSort";
import { useWorkspaceStore } from "../stores/workspaceStore";
import "../styles/tabbar.scss";

const props = defineProps({
  tabs: { type: Array, required: true },
  activeId: { type: String, default: null },
  allowCreateConnection: { type: Boolean, default: true },
  connections: { type: Array, required: true },
  connectProtocolFilter: { type: String, default: "" },
  newConnectionLabel: { type: String, default: "New connection" },
  noSessionsLabel: { type: String, default: "No active sessions" },
  closeLabel: { type: String, default: "Close" },
  sideButtons: { type: Array, default: () => [] },
});

const { sessionRuntime } = useWorkspaceStore();
const tabsWithRuntime = computed(() =>
  props.tabs.map((tab) => ({ ...tab, ...sessionRuntime(tab.id) })),
);

const emit = defineEmits([
  "select",
  "close",
  "connect",
  "connection-created",
  "reorder",
  "side-action",
]);

const connectDialogOpen = ref(false);
const closeIconProps = { size: 12, strokeWidth: 2 };
const addIconProps = { size: 14, strokeWidth: 2 };

const {
  dragging,
  isSelectionSuppressed,
  listRef: tabListRef,
} = useWorkspaceTabDragSort({
  getTabIds: currentTabIds,
  onReorder: (nextOrder) => emit("reorder", nextOrder),
});

watch(
  () => props.activeId,
  () => {
    if (dragging.value) return;
    nextTick(() => {
      const container = tabListRef.value?.$el || tabListRef.value;
      if (!container) return;
      const active = container.querySelector(".workspace-session-tab-item-active");
      if (!active) return;
      const targetScroll = active.offsetLeft - (container.clientWidth - active.offsetWidth) / 2;
      container.scrollTo({ left: targetScroll, behavior: "smooth" });
    });
  },
);

function statusClass(status) {
  if (status === "online") return "ui-status-dot-online";
  if (status === "warning") return "ui-status-dot-warning";
  return "ui-status-dot-offline";
}

function currentTabIds() {
  return props.tabs.map((tab) => tab.id);
}

function selectTab(id) {
  if (!id || isSelectionSuppressed()) return;
  emit("select", id);
}

function closeTab(e, id) {
  e.preventDefault();
  e.stopPropagation();
  if (dragging.value) return;
  emit("close", id);
}

// ── Marquee scroll on hover ──
const MARQUEE_SPEED = 20; // px/s — lower = gentler scroll

function onLabelMouseEnter(e) {
  if (dragging.value) return;
  const label = e.currentTarget;
  const track = label.firstElementChild;
  if (!track) return;

  // Only Copy1 is visible (Copy2 is display:none), so scrollWidth == one copy width
  if (track.scrollWidth <= label.clientWidth) return;

  const duration = Math.max(3, (track.scrollWidth + 20) / MARQUEE_SPEED);
  track.style.setProperty("--marquee-duration", `${duration}s`);
  track.classList.add("is-overflow");
}

function onLabelMouseLeave(e) {
  const track = e.currentTarget.firstElementChild;
  if (!track) return;
  track.classList.remove("is-overflow");
  track.style.removeProperty("--marquee-duration");
}

function openConnectDialog() {
  connectDialogOpen.value = true;
}
</script>

<template>
  <TabsRoot
    :model-value="activeId || undefined"
    class="workspace-tabbar"
    orientation="horizontal"
    activation-mode="manual"
    @update:model-value="selectTab"
  >
    <TabsList
      ref="tabListRef"
      as="div"
      class="workspace-tabbar-scroll"
      :class="{ 'workspace-tabbar-scroll-dragging': dragging }"
    >
      <TabsTrigger
        v-for="tab in tabsWithRuntime"
        :key="tab.id"
        as="div"
        :value="tab.id"
        class="ui-session-tab workspace-session-tab-item"
        :class="{ 'ui-session-tab-active workspace-session-tab-item-active': tab.id === activeId }"
        :data-id="tab.id"
      >
        <!-- Zone 1: Status indicator -->
        <span
          :class="['ui-status-dot', statusClass(tab.status)]"
          aria-hidden="true"
        />

        <!-- Zone 2: Connection label (twin-copy marquee) -->
        <span
          class="session-tab-label"
          @mouseenter="onLabelMouseEnter"
          @mouseleave="onLabelMouseLeave"
        >
          <span class="session-tab-label-track">
            <span class="session-tab-label-text">{{ tab.name }}</span>
            <span
              class="session-tab-label-text"
              aria-hidden="true"
            >{{ tab.name }}</span>
          </span>
        </span>

        <!-- Zone 3: Close button -->
        <button
          type="button"
          class="tab-close-btn"
          :aria-label="closeLabel"
          @pointerdown.stop
          @mousedown.stop
          @click="closeTab($event, tab.id)"
        >
          <X v-bind="closeIconProps" />
        </button>
      </TabsTrigger>

      <template v-if="tabsWithRuntime.length === 0">
        <div class="workspace-empty-tabs">
          <span>{{ noSessionsLabel }}</span>
        </div>
      </template>
    </TabsList>

    <AppTooltip
      :content="newConnectionLabel"
      side="bottom"
    >
      <button
        type="button"
        class="workspace-tabbar-tool-button"
        :aria-label="newConnectionLabel"
        @click="openConnectDialog"
      >
        <Plus v-bind="addIconProps" />
      </button>
    </AppTooltip>

    <template v-if="sideButtons.length">
      <div class="workspace-tabbar-tools">
        <AppTooltip
          v-for="button in sideButtons"
          :key="button.id"
          :content="button.label"
          side="bottom"
        >
          <button
            type="button"
            class="workspace-tabbar-tool-button"
            :class="{ 'workspace-tabbar-tool-button-active': button.active }"
            :aria-label="button.label"
            :disabled="button.disabled"
            @click="emit('side-action', button.id)"
          >
            <component :is="button.icon" />
          </button>
        </AppTooltip>
      </div>
    </template>

    <ConnectDialog
      :open="connectDialogOpen"
      :allow-create-connection="allowCreateConnection"
      :connections="connections"
      :protocol-filter="connectProtocolFilter"
      @update:open="connectDialogOpen = $event"
      @connect="emit('connect', $event)"
      @connection-created="emit('connection-created', $event)"
    />
  </TabsRoot>
</template>
