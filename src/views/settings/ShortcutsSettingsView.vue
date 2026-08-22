<script setup>
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { Keyboard } from "@lucide/vue";
import { storeToRefs } from "pinia";
import { useWorkspaceStore } from "../../stores/workspaceStore";

const { t } = useI18n();
const { preferences } = storeToRefs(useWorkspaceStore());
const shortcutCapture = ref("");

const shortcutRows = [
  ["terminalSearchShortcut", "terminalSearch", "terminalSearchHint"],
  ["serialRedetectBaudShortcut", "serialRedetectBaud", "serialRedetectBaudHint"],
  ["sessionRecordingShortcut", "sessionRecording", "sessionRecordingHint"],
  ["openDevToolsShortcut", "openDevTools", "openDevToolsHint"],
];

function shortcutKeyLabel(event) {
  if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return "";
  if (event.code === "Space" || event.key === " ") return "Space";
  if (event.key.length === 1) return event.key.toUpperCase();
  return event.key;
}

function formatShortcutEvent(event) {
  const parts = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");
  const key = shortcutKeyLabel(event);
  if (key) parts.push(key);
  return key ? parts.join("+") : "";
}

function captureShortcut(event, preferenceKey) {
  event.preventDefault();
  event.stopPropagation();
  if (event.key === "Escape") {
    shortcutCapture.value = "";
    event.currentTarget?.blur?.();
    return;
  }
  if (event.key === "Backspace" || event.key === "Delete") {
    preferences.value[preferenceKey] = "";
    shortcutCapture.value = "";
    return;
  }
  const shortcut = formatShortcutEvent(event);
  if (!shortcut) return;
  preferences.value[preferenceKey] = shortcut;
  shortcutCapture.value = "";
  event.currentTarget?.blur?.();
}
</script>

<template>
  <section class="settings-section">
    <div class="settings-section-header">
      <Keyboard
        :size="16"
        stroke-width="1.8"
        class="text-accent"
      />
      <div>
        <h3 class="settings-section-title">
          {{ t("settings.shortcuts.title") }}
        </h3>
        <p class="settings-section-desc">
          {{ t("settings.shortcuts.hint") }}
        </p>
      </div>
    </div>
    <div class="settings-fields">
      <div
        v-for="row in shortcutRows"
        :key="row[0]"
        class="settings-toggle"
      >
        <div>
          <span class="settings-label">{{ t(`settings.shortcuts.${row[1]}`) }}</span>
          <span class="settings-hint">{{ t(`settings.shortcuts.${row[2]}`) }}</span>
        </div>
        <button
          type="button"
          class="shortcut-capture"
          :class="{ 'shortcut-capture-active': shortcutCapture === row[0] }"
          @click="shortcutCapture = row[0]"
          @focus="shortcutCapture = row[0]"
          @blur="shortcutCapture = ''"
          @keydown="captureShortcut($event, row[0])"
        >
          {{
            shortcutCapture === row[0]
              ? t("settings.shortcuts.recording")
              : preferences[row[0]] || t("settings.shortcuts.unassigned")
          }}
        </button>
      </div>
    </div>
  </section>
</template>
