<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import { emitTo as importedEmitTo, listen as importedListen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { commitLocale, currentLocale, loadLocaleMessages, resolveLocale } from "../i18n";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { CONTEXT_MENU_EVENTS, CONTEXT_MENU_LAYOUT } from "../utils/contextMenu";
import { addDomListener } from "../utils/domListeners";
import ContextMenuPanel from "./ContextMenuPanel.vue";

const menuRef = ref(null);
const requestId = ref(0);
const items = shallowRef([]);
const menuOpen = ref(false);
const theme = ref("light");
const menuPanelWidth = ref(CONTEXT_MENU_LAYOUT.width);
const menuMaxHeight = ref(CONTEXT_MENU_LAYOUT.maxHeight);

let closeInFlight = false;
let closeResetTimer = 0;
let localeApplySequence = 0;
const asyncListeners = createAsyncListenerRegistry();

const menuStyle = computed(() => ({
  "--context-menu-panel-width": `${menuPanelWidth.value}px`,
  "--context-menu-max-height": `${menuMaxHeight.value}px`,
}));

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
  <ContextMenuPanel
    v-if="menuOpen && items.length"
    ref="menuRef"
    :items="items"
    :theme="theme"
    :style="menuStyle"
    @click.self="closeMenu('dismiss')"
    @keydown.escape.stop.prevent="closeMenu('keyboard')"
    @activate="activateItem"
  />
</template>
