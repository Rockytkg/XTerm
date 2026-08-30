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
import { FileCode, Play, Square } from "@lucide/vue";
import { useScriptExecution } from "../composables/useScriptExecution";
import { useToasts } from "../composables/useToasts";
import { useScriptsStore } from "../stores/scriptsStore";
import { closeScriptRunPicker, scriptRunPickerOpen } from "../services/scripting/scriptRunPicker";
import { SCRIPT_RUN_STATUS, scriptRuns, stopScript } from "../services/scripting/scriptRunner";

const { t } = useI18n();
const router = useRouter();
const scriptsStore = useScriptsStore();
const { scripts } = storeToRefs(scriptsStore);
const { runScriptOnActiveSession } = useScriptExecution();
const { showToast } = useToasts();

const sortedScripts = computed(() =>
  [...scripts.value].sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0)),
);
const runningScriptIds = computed(
  () =>
    new Set(
      scriptRuns.value
        .filter((run) => run.status === SCRIPT_RUN_STATUS.RUNNING)
        .map((run) => run.scriptId),
    ),
);

onMounted(() => {
  if (!scriptsStore.loaded) void scriptsStore.loadScripts();
});

function onOpenChange(open) {
  if (!open) closeScriptRunPicker();
}

function runScript(script) {
  // 已在运行的脚本不允许重复启动，但仍需响应点击以关闭选择弹窗。
  const runningRun = scriptRuns.value.find(
    (run) => run.scriptId === script.id && run.status === SCRIPT_RUN_STATUS.RUNNING,
  );
  if (runningRun) {
    closeScriptRunPicker();
    void stopRunningScript(runningRun);
    return;
  }
  closeScriptRunPicker();
  void runScriptOnActiveSession(script);
}

function goManageScripts() {
  closeScriptRunPicker();
  router.push({ name: "scripts" });
}

async function stopRunningScript(run) {
  showToast({ type: "info", title: t("notifications.scriptStopping", { name: run.scriptName }) });
  if (!stopScript(run.runId)) {
    showToast({ type: "error", title: t("notifications.scriptStopFailed", { name: run.scriptName }) });
    return;
  }
  for (let attempt = 0; attempt < 30 && run.status === SCRIPT_RUN_STATUS.RUNNING; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  if (run.status === SCRIPT_RUN_STATUS.STOPPED) {
    showToast({ type: "success", title: t("notifications.scriptStopped", { name: run.scriptName }) });
  } else {
    showToast({ type: "error", title: t("notifications.scriptStopFailed", { name: run.scriptName }) });
  }
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
            :class="{ 'is-running': runningScriptIds.has(script.id) }"
            :title="
              runningScriptIds.has(script.id)
                ? t('scripts.picker.stopHint')
                : t('scripts.picker.runHint')
            "
            @click="runScript(script)"
          >
            <span class="script-picker-item-name">{{ script.name || t("scripts.untitled") }}</span>
            <span class="script-picker-item-action">
              <span
                v-if="runningScriptIds.has(script.id)"
                class="script-picker-item-status"
              >
                {{ t("scripts.picker.running") }}
              </span>
              <Square
                v-if="runningScriptIds.has(script.id)"
                :size="13"
                stroke-width="2.2"
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
