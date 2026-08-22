<script setup>
import {
  CollapsibleContent,
  CollapsibleRoot,
  CollapsibleTrigger,
  ProgressIndicator,
  ProgressRoot,
} from "reka-ui";
import {
  AlertCircle,
  CheckCircle2,
  ChevronUp,
  Download,
  FolderSync,
  LoaderCircle,
  Pause,
  Play,
  Upload,
  X,
} from "@lucide/vue";
import { formatBytes } from "../../utils/formatBytes";

defineProps({
  activeTransfers: { type: Number, default: 0 },
  cancelLabel: { type: String, required: true },
  clearCompletedLabel: { type: String, required: true },
  collapseLabel: { type: String, required: true },
  completedLabel: { type: String, required: true },
  completedTransfers: { type: Number, default: 0 },
  expandLabel: { type: String, required: true },
  isCollapsed: { type: Boolean, default: true },
  noTransfersLabel: { type: String, required: true },
  runningLabel: { type: String, required: true },
  pauseLabel: { type: String, required: true },
  resumeLabel: { type: String, required: true },
  setQueueListRef: { type: Function, default: () => {} },
  title: { type: String, required: true },
  transfers: { type: Array, default: () => [] },
});

defineEmits(["cancel", "clearCompleted", "pause", "remove", "resume", "update:isCollapsed"]);

const controllableStatuses = new Set(["running", "pausing", "paused"]);

function isControllable(status) {
  return controllableStatuses.has(status);
}
</script>

<template>
  <CollapsibleRoot
    as="aside"
    class="sftp-queue"
    :class="{ 'sftp-queue-collapsed': isCollapsed }"
    :open="!isCollapsed"
    @update:open="$emit('update:isCollapsed', !$event)"
  >
    <div class="sftp-queue-heading">
      <CollapsibleTrigger as-child>
        <button
          type="button"
          class="sftp-queue-toggle"
          :aria-label="isCollapsed ? expandLabel : collapseLabel"
          :title="isCollapsed ? expandLabel : collapseLabel"
        >
          <ChevronUp
            :size="14"
            stroke-width="2"
            class="sftp-queue-toggle-icon"
          />
        </button>
      </CollapsibleTrigger>
      <div class="min-w-0 flex items-baseline gap-[8px]">
        <strong class="text-[0.8571em]">{{ title }}</strong>
        <span class="text-[0.7857em] text-text-tertiary">
          {{ activeTransfers }} {{ runningLabel }} / {{ completedTransfers }} {{ completedLabel }}
        </span>
      </div>
      <button
        type="button"
        class="sftp-text-button"
        :disabled="!transfers.length"
        @click="$emit('clearCompleted')"
      >
        {{ clearCompletedLabel }}
      </button>
    </div>

    <CollapsibleContent>
      <div
        v-if="!transfers.length"
        class="sftp-queue-empty"
      >
        <FolderSync
          :size="16"
          stroke-width="1.8"
        />
        <span>{{ noTransfersLabel }}</span>
      </div>
      <div
        v-else
        :ref="setQueueListRef"
        class="sftp-queue-list"
      >
        <div
          v-for="item in transfers"
          :key="item.id"
          class="sftp-queue-item"
          :class="`is-${item.status}`"
          :data-transfer-id="item.id"
        >
          <div
            class="sftp-queue-row"
            :class="{
              'sftp-queue-item-done': item.status === 'done',
              'sftp-queue-item-failed': item.status === 'failed',
            }"
          >
            <Upload
              v-if="item.direction === 'upload'"
              :size="14"
              stroke-width="1.9"
            />
            <Download
              v-else
              :size="14"
              stroke-width="1.9"
            />
            <span class="sftp-queue-name-wrap">
              <span class="sftp-queue-name">{{ item.name }}</span>
              <LoaderCircle
                v-if="item.status === 'running' || item.status === 'pausing'"
                :size="13"
                stroke-width="1.9"
                class="animate-spin"
              />
              <CheckCircle2
                v-else-if="item.status === 'done'"
                :size="13"
                stroke-width="1.9"
              />
              <Pause
                v-else-if="item.status === 'paused'"
                :size="13"
                stroke-width="1.9"
              />
              <AlertCircle
                v-else
                :size="13"
                stroke-width="1.9"
              />
            </span>
            <div class="sftp-queue-actions">
              <button
                v-if="item.status === 'running'"
                type="button"
                class="sftp-queue-action"
                :aria-label="pauseLabel"
                :title="pauseLabel"
                @click="$emit('pause', item.id)"
              >
                <Pause
                  :size="11"
                  stroke-width="2"
                />
              </button>
              <button
                v-else-if="item.status === 'paused'"
                type="button"
                class="sftp-queue-action"
                :aria-label="resumeLabel"
                :title="resumeLabel"
                @click="$emit('resume', item.id)"
              >
                <Play
                  :size="11"
                  stroke-width="2"
                />
              </button>
              <button
                v-if="isControllable(item.status)"
                type="button"
                class="sftp-queue-action sftp-queue-action-danger"
                :aria-label="cancelLabel"
                :title="cancelLabel"
                @click="$emit('cancel', item.id)"
              >
                <X
                  :size="11"
                  stroke-width="2"
                />
              </button>
              <button
                v-else
                type="button"
                class="sftp-queue-close"
                @click="$emit('remove', item.id)"
              >
                <X
                  :size="11"
                  stroke-width="2"
                />
              </button>
            </div>
          </div>
          <div class="sftp-queue-meta">
            <span class="min-w-0 truncate">
              {{
                item.error ||
                  `${formatBytes(item.transferred)} / ${formatBytes(item.total)} · ${item.speed}`
              }}
            </span>
            <span class="sftp-queue-progress-pct">{{ Math.floor(Number(item.progress) || 0) }}%</span>
          </div>
          <ProgressRoot
            class="sftp-progress-track"
            :model-value="Number(item.progress)"
          >
            <ProgressIndicator
              class="sftp-progress-bar"
              :class="{
                'bg-success': item.status === 'done',
                'bg-danger': item.status === 'failed',
                'sftp-progress-bar-active': item.status === 'running' || item.status === 'pausing',
              }"
              :style="{
                '--sftp-progress-scale': Math.max(
                  0,
                  Math.min(1, (Number(item.progress) || 0) / 100),
                ),
              }"
            />
          </ProgressRoot>
        </div>
      </div>
    </CollapsibleContent>
  </CollapsibleRoot>
</template>
