<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import { useI18n } from "vue-i18n";
import { emitTo as importedEmitTo, listen as importedListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commitLocale, currentLocale, loadLocaleMessages, resolveLocale } from "../i18n";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { CONTEXT_MENU_EVENTS, CONTEXT_MENU_LAYOUT } from "../utils/contextMenu";
import { addDomListener } from "../utils/domListeners";
import "../styles/context-menu.scss";
import {
  ClipboardCopy,
  ClipboardPaste,
  Copy,
  Download,
  Eraser,
  FilePenLine,
  FilePlus,
  FolderOpen,
  FolderPlus,
  ListChecks,
  RefreshCw,
  RotateCcw,
  RotateCw,
  Scissors,
  Search,
  Terminal,
  TextSelect,
  Trash2,
  Upload,
} from "@lucide/vue";

const ICONS = {
  clear: Eraser,
  copy: Copy,
  copyPath: ClipboardCopy,
  cut: Scissors,
  delete: Trash2,
  download: Download,
  edit: FilePenLine,
  newFile: FilePlus,
  newFolder: FolderPlus,
  open: FolderOpen,
  paste: ClipboardPaste,
  redo: RotateCw,
  refresh: RefreshCw,
  rename: FilePenLine,
  search: Search,
  selectAll: TextSelect,
  terminal: Terminal,
  terminalSelectAll: ListChecks,
  undo: RotateCcw,
  upload: Upload,
};

const DEFAULT_ICON_BY_ID = {
  "global-copy": "copy",
  "global-cut": "cut",
  "global-delete": "delete",
  "global-paste": "paste",
  "global-redo": "redo",
  "global-select-all": "selectAll",
  "global-undo": "undo",
  "relationship-edit": "edit",
  "relationship-focus-related": "search",
  "relationship-quick-add": "redo",
  "relationship-refresh": "refresh",
  "relationship-delete-credential": "delete",
  "relationship-remove-relation": "delete",
  "sftp-copy-path": "copyPath",
  "sftp-delete": "delete",
  "sftp-download": "download",
  "sftp-edit": "edit",
  "sftp-new-file": "newFile",
  "sftp-new-folder": "newFolder",
  "sftp-open": "open",
  "sftp-refresh": "refresh",
  "sftp-rename": "rename",
  "sftp-select-all": "selectAll",
  "sftp-upload": "upload",
  "terminal-clear": "clear",
  "terminal-search": "search",
  "terminal-select-all": "terminalSelectAll",
};

const menuRef = ref(null);
const requestId = ref(0);
const items = shallowRef([]);
const menuOpen = ref(false);
const theme = ref("light");
const menuPanelWidth = ref(CONTEXT_MENU_LAYOUT.width);
const menuMaxHeight = ref(CONTEXT_MENU_LAYOUT.maxHeight);
const { t } = useI18n();

let closeInFlight = false;
let closeResetTimer = 0;
let localeApplySequence = 0;
const asyncListeners = createAsyncListenerRegistry();

const menuStyle = computed(() => ({
  "--context-menu-panel-width": `${menuPanelWidth.value}px`,
  "--context-menu-max-height": `${menuMaxHeight.value}px`,
}));

function iconFor(item) {
  return ICONS[item.icon || DEFAULT_ICON_BY_ID[item.id] || "terminal"] || Terminal;
}

function isDangerItem(item) {
  return item?.tone === "danger" || item?.id?.includes("delete");
}

function itemLabel(item) {
  return item?.labelKey ? t(item.labelKey) : item?.label || "";
}

function tauriEventApi() {
  return window.__TAURI__?.event;
}

function listenEvent(eventName, handler) {
  return (tauriEventApi()?.listen || importedListen)(eventName, handler);
}

function emitToWindow(windowLabel, eventName, payload) {
  return (tauriEventApi()?.emitTo || importedEmitTo)(windowLabel, eventName, payload);
}

function closeMenu(reason = "dismiss") {
  if (closeInFlight) return;
  closeInFlight = true;
  hideRenderedMenu();
  window.clearTimeout(closeResetTimer);
  if (requestId.value) {
    emitToWindow("main", CONTEXT_MENU_EVENTS.close, {
      reason,
      requestId: requestId.value,
    });
  }
  getCurrentWindow().hide().finally(resetCloseGuardSoon);
}

function resetCloseGuardSoon() {
  window.clearTimeout(closeResetTimer);
  closeResetTimer = window.setTimeout(() => {
    closeInFlight = false;
    closeResetTimer = 0;
  }, 0);
}

function hideRenderedMenu() {
  menuOpen.value = false;
}

async function activateItem(item) {
  if (!item?.enabled) return;
  closeInFlight = true;
  hideRenderedMenu();
  try {
    await emitToWindow("main", CONTEXT_MENU_EVENTS.action, {
      requestId: requestId.value,
      id: item.id,
    });
  } finally {
    getCurrentWindow().hide().finally(resetCloseGuardSoon);
  }
}

async function applyPayloadLocale(payloadLocale) {
  if (typeof payloadLocale !== "string" || !payloadLocale.trim()) return;
  const nextLocale = resolveLocale(payloadLocale);
  if (currentLocale() === nextLocale) return;
  const sequence = ++localeApplySequence;
  await loadLocaleMessages(nextLocale);
  if (sequence === localeApplySequence) {
    commitLocale(nextLocale);
  }
}

async function openMenu(payload) {
  closeInFlight = false;
  window.clearTimeout(closeResetTimer);
  closeResetTimer = 0;
  await applyPayloadLocale(payload?.locale);
  requestId.value = payload?.requestId || 0;
  items.value = Array.isArray(payload?.items) ? payload.items.filter(Boolean) : [];
  theme.value = payload?.theme === "dark" ? "dark" : "light";
  menuPanelWidth.value = Number(payload?.width) || CONTEXT_MENU_LAYOUT.width;
  menuMaxHeight.value = Number(payload?.maxHeight) || CONTEXT_MENU_LAYOUT.maxHeight;
  document.documentElement.dataset.theme = theme.value;
  document.documentElement.style.colorScheme = theme.value;
  if (!items.value.length) return;
  menuOpen.value = true;
  nextTick(() => menuRef.value?.focus?.());
}

function preventNativeContextMenu(event) {
  event.preventDefault();
  event.stopPropagation();
}

onMounted(async () => {
  asyncListeners.add(addDomListener(document, "contextmenu", preventNativeContextMenu, true));

  await asyncListeners.register(
    listenEvent(CONTEXT_MENU_EVENTS.open, ({ payload }) => {
      openMenu(payload);
    }),
  );

  await asyncListeners.register(
    getCurrentWindow().onFocusChanged(({ payload }) => {
      if (!payload) closeMenu("blur");
    }),
  );

  await emitToWindow("main", CONTEXT_MENU_EVENTS.ready);
});

onBeforeUnmount(() => {
  asyncListeners.dispose();
  window.clearTimeout(closeResetTimer);
});
</script>

<template>
  <span class="floating-context-menu-anchor" />
  <main
    v-if="menuOpen && items.length"
    ref="menuRef"
    class="floating-context-menu ui-context-menu-shell"
    :data-theme="theme"
    :style="menuStyle"
    aria-label="Context menu"
    role="menu"
    tabindex="-1"
    @click.self="closeMenu('dismiss')"
    @keydown.escape.stop.prevent="closeMenu('keyboard')"
  >
    <template
      v-for="(item, index) in items"
      :key="item.id || `separator-${index}`"
    >
      <div
        v-if="item.type === 'separator'"
        class="ui-context-menu-separator"
        role="separator"
      />
      <button
        v-else
        type="button"
        class="ui-context-menu-item"
        :class="{ 'is-danger': isDangerItem(item) }"
        :disabled="!item.enabled"
        :data-disabled="!item.enabled ? '' : undefined"
        role="menuitem"
        @click="activateItem(item)"
      >
        <span
          class="ui-context-menu-icon"
          aria-hidden="true"
        >
          <component
            :is="iconFor(item)"
            :size="19"
            stroke-width="2.05"
          />
        </span>
        <span class="ui-context-menu-label">{{ itemLabel(item) }}</span>
        <kbd
          v-if="item.shortcut"
          class="ui-context-menu-shortcut"
        >{{ item.shortcut }}</kbd>
      </button>
    </template>
  </main>
</template>
