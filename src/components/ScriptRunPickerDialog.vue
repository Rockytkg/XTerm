<script setup>
import { computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { storeToRefs } from "pinia";
import {
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui";
import { Clock, FileCode, Play, Square } from "@lucide/vue";
import { useScriptExecution } from "../composables/useScriptExecution";
import { useWorkspaceStore } from "../stores/workspaceStore";
import { useScriptsStore } from "../stores/scriptsStore";
import { closeScriptRunPicker, scriptRunPickerOpen } from "../services/scripting/scriptRunPicker";
import { SCRIPT_RUN_STATUS, scriptRuns } from "../services/scripting/scriptRunner";

const { t } = useI18n();
const router = useRouter();
const workspace = useWorkspaceStore();
const scriptsStore = useScriptsStore();
const { scripts } = storeToRefs(scriptsStore);
const { runScriptOnActiveSession, stopScriptRun } = useScriptExecution();

const sortedScripts = computed(() =>
  [...scripts.value].sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0)),
);
const activeSessionId = computed(() => workspace.activeConnection || "");
const activeRunsByScriptId = computed(() => {
  const activeRuns = new Map();
  for (const run of scriptRuns.value) {
    if (
      (run.status === SCRIPT_RUN_STATUS.RUNNING || run.status === SCRIPT_RUN_STATUS.QUEUED) &&
      run.targetSessionId === activeSessionId.value &&
      !activeRuns.has(run.scriptId)
    ) {
      activeRuns.set(run.scriptId, run);
    }
  }
  return activeRuns;
});

onMounted(() => {
  if (!scriptsStore.loaded) void scriptsStore.loadScripts();
});

function onOpenChange(open) {
  if (!open) closeScriptRunPicker();
}

function runScript(script) {
  const activeRun = activeRunsByScriptId.value.get(script.id);
  if (activeRun) {
    closeScriptRunPicker();
    void stopScriptRun(activeRun);
    return;
  }
  closeScriptRunPicker();
  void runScriptOnActiveSession(script);
}

function goManageScripts() {
  closeScriptRunPicker();
  router.push({ name: "scripts" });
}
</script>

<template>
  <DialogRoot
    :open="scriptRunPickerOpen"
    @update:open="onOpenChange"
  >
    <DialogPortal>
      <DialogOverlay class="dialog-overlay conn-dialog-overlay" />
      <DialogContent class="dialog-content conn-dialog focus:outline-none">
        <header class="conn-dialog-header">
          <div
            class="conn-dialog-header-icon"
            aria-hidden="true"
          >
            <FileCode
              :size="16"
              stroke-width="1.8"
            />
          </div>
          <div class="flex-1 min-w-0">
            <DialogTitle class="conn-dialog-title">
              {{ t("scripts.picker.title") }}
            </DialogTitle>
            <DialogDescription class="conn-dialog-desc">
              {{ t("scripts.picker.description") }}
            </DialogDescription>
          </div>
        </header>

        <div class="conn-dialog-body script-picker-body">
          <div
            v-if="!sortedScripts.length"
            class="ui-empty-state px-[16px] py-[28px] text-[0.8571em]"
          >
            <p>{{ t("scripts.picker.empty") }}</p>
          </div>
          <button
            v-for="script in sortedScripts"
            v-else
            :key="script.id"
            type="button"
            class="script-picker-item"
            :class="{
              'is-running':
                activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.RUNNING,
              'is-queued': activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.QUEUED,
            }"
            :title="
              activeRunsByScriptId.has(script.id)
                ? activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.QUEUED
                  ? t('scripts.picker.cancelQueueHint')
                  : t('scripts.picker.stopHint')
                : t('scripts.picker.runHint')
            "
            @click="runScript(script)"
          >
            <span class="script-picker-item-name">{{ script.name || t("scripts.untitled") }}</span>
            <span class="script-picker-item-action">
              <span
                v-if="activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.RUNNING"
                class="script-picker-item-status"
              >
                {{ t("scripts.picker.running") }}
              </span>
              <span
                v-else-if="activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.QUEUED"
                class="script-picker-item-status"
              >
                {{ t("scripts.picker.queued") }}
              </span>
              <Square
                v-if="activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.RUNNING"
                :size="13"
                stroke-width="2.2"
              />
              <Clock
                v-else-if="activeRunsByScriptId.get(script.id)?.status === SCRIPT_RUN_STATUS.QUEUED"
                :size="13"
                stroke-width="2"
              />
              <Play
                v-else
                :size="13"
                stroke-width="2"
              />
            </span>
          </button>
        </div>

        <footer class="conn-dialog-footer">
          <button
            type="button"
            class="ui-button-secondary"
            @click="goManageScripts"
          >
            {{ t("scripts.picker.manage") }}
          </button>
          <button
            type="button"
            class="ui-button-primary"
            @click="closeScriptRunPicker"
          >
            {{ t("actions.close") }}
          </button>
        </footer>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
.script-picker-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 320px;
  overflow-y: auto;
}

.script-picker-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border-light);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
}

.script-picker-item:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--accent);
}

.script-picker-item.is-running {
  border-color: color-mix(in srgb, var(--danger) 55%, var(--border-light));
  color: var(--danger);
}

.script-picker-item.is-queued {
  border-color: color-mix(in srgb, var(--warning) 55%, var(--border-light));
  color: var(--text-primary);
}

.script-picker-item.is-running:hover {
  border-color: var(--danger);
  background: color-mix(in srgb, var(--danger) 8%, var(--bg-secondary));
  color: var(--danger) !important;
}

.script-picker-item-action {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: none;
}

.script-picker-item-status {
  font-size: 0.78em;
  font-weight: 600;
}

.script-picker-item:disabled {
  opacity: 0.5;
  cursor: default;
}

.script-picker-item-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
