<script setup>
import { computed, onBeforeUnmount, ref } from "vue";
import { File, Folder } from "@lucide/vue";
import { formatBytes } from "../../utils/formatBytes";
import { createRafThrottle } from "../../utils/schedulers";
import { iconForSftpEntry } from "../../utils/sftpEntryPresentation";

const ROW_HEIGHT = 34;
const OVERSCAN_ROWS = 8;
const VIRTUALIZE_AFTER = 80;

const props = defineProps({
  cancelInlineEdit: { type: Function, required: true },
  commitInlineEdit: { type: Function, required: true },
  creatingEntry: { type: Boolean, default: false },
  creatingFolder: { type: Boolean, default: false },
  dropTargetPath: { type: String, default: "" },
  fileTypeLabel: { type: Function, required: true },
  filteredRemoteFiles: { type: Array, default: () => [] },
  formatModified: { type: Function, required: true },
  inlineEdit: { type: Object, required: true },
  isEditingEntry: { type: Function, required: true },
  labels: { type: Object, required: true },
  loading: { type: Boolean, default: false },
  moveDropTargetPath: { type: String, default: "" },
  parentDirectoryEntry: { type: Object, default: null },
  selectedNames: { type: Object, required: true },
  setInlineEditInputRef: { type: Function, default: () => {} },
  setTableBodyRef: { type: Function, default: () => {} },
});

defineEmits([
  "clearSelection",
  "domDragLeave",
  "domDragOver",
  "domDrop",
  "moveMouseDown",
  "openEntry",
  "openParent",
  "selectEntry",
  "startRenameEntry",
  "suppressMoveClick",
  "updateInlineEditValue",
]);

const scrollTop = ref(0);
const viewportHeight = ref(0);
let browserElement = null;
let resizeObserver = null;
const scheduleViewportUpdate = createRafThrottle(applyViewport);

const rows = computed(() => {
  const next = [];
  if (props.parentDirectoryEntry) {
    next.push({ type: "parent", key: "parent", entry: props.parentDirectoryEntry });
  }
  if (props.creatingEntry) {
    next.push({ type: "creating", key: "creating" });
  }
  for (const entry of props.filteredRemoteFiles) {
    next.push({ type: "entry", key: entry.path, entry });
  }
  return next;
});

const shouldVirtualize = computed(() => !props.loading && rows.value.length > VIRTUALIZE_AFTER);
const visibleRange = computed(() => {
  if (!shouldVirtualize.value) {
    return { start: 0, end: rows.value.length, top: 0, bottom: 0 };
  }

  const firstVisible = Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT) - OVERSCAN_ROWS);
  const visibleCount =
    Math.ceil(Math.max(ROW_HEIGHT, viewportHeight.value - ROW_HEIGHT) / ROW_HEIGHT) +
    OVERSCAN_ROWS * 2;
  const end = Math.min(rows.value.length, firstVisible + visibleCount);
  return {
    start: firstVisible,
    end,
    top: firstVisible * ROW_HEIGHT,
    bottom: Math.max(0, rows.value.length - end) * ROW_HEIGHT,
  };
});

const visibleRows = computed(() =>
  rows.value.slice(visibleRange.value.start, visibleRange.value.end),
);
const isEmpty = computed(
  () => !props.parentDirectoryEntry && !props.filteredRemoteFiles.length && !props.creatingEntry,
);

function applyViewport(element = browserElement) {
  if (!element) return;
  scrollTop.value = element.scrollTop;
  viewportHeight.value = element.clientHeight;
}

function updateViewport(event) {
  browserElement = event.currentTarget;
  scheduleViewportUpdate();
}

function disconnectResizeObserver() {
  resizeObserver?.disconnect();
  resizeObserver = null;
}

function setBrowserRef(element) {
  if (browserElement === element) return;
  disconnectResizeObserver();
  browserElement = element;
  if (!element) return;
  applyViewport(element);
  if (typeof ResizeObserver === "undefined") return;
  resizeObserver = new ResizeObserver(() => scheduleViewportUpdate(element));
  resizeObserver.observe(element);
}

onBeforeUnmount(() => {
  disconnectResizeObserver();
  scheduleViewportUpdate.cancel();
});
</script>

<template>
  <main
    :ref="setBrowserRef"
    class="sftp-browser"
    @click="$emit('clearSelection', $event)"
    @dragover.prevent="$emit('domDragOver', $event)"
    @dragleave="$emit('domDragLeave', $event)"
    @drop="$emit('domDrop', $event)"
    @scroll="updateViewport"
  >
    <span
      v-if="loading"
      class="sftp-skeleton-status"
      role="status"
      aria-live="polite"
    >{{
      labels.loading
    }}</span>
    <div
      class="sftp-table"
      role="table"
    >
      <div
        class="sftp-table-head"
        role="rowgroup"
      >
        <div
          class="sftp-header-row"
          role="row"
        >
          <div role="columnheader">
            {{ labels.name }}
          </div>
          <div role="columnheader">
            {{ labels.type }}
          </div>
          <div role="columnheader">
            {{ labels.size }}
          </div>
          <div
            role="columnheader"
            class="sftp-compact-hidden"
          >
            {{ labels.modified }}
          </div>
        </div>
      </div>
      <div
        :ref="setTableBodyRef"
        class="sftp-table-body"
        role="rowgroup"
        :aria-busy="loading ? 'true' : 'false'"
        @mousedown.capture="$emit('moveMouseDown', $event)"
        @click.capture="$emit('suppressMoveClick', $event)"
      >
        <template v-if="loading">
          <div
            v-for="row in 10"
            :key="`sftp-skeleton-${row}`"
            class="sftp-row sftp-skeleton-row"
            role="row"
            aria-hidden="true"
          >
            <div role="cell">
              <span class="sftp-name-cell">
                <span class="sftp-skeleton-icon" />
                <span class="sftp-skeleton-bar sftp-skeleton-name" />
              </span>
            </div>
            <div role="cell">
              <span class="sftp-skeleton-bar sftp-skeleton-type" />
            </div>
            <div role="cell">
              <span class="sftp-skeleton-bar sftp-skeleton-size" />
            </div>
            <div
              role="cell"
              class="sftp-compact-hidden"
            >
              <span class="sftp-skeleton-bar sftp-skeleton-modified" />
            </div>
          </div>
        </template>
        <div
          v-else-if="isEmpty"
          class="sftp-state-row"
          role="row"
        >
          <div
            class="sftp-state-cell"
            role="cell"
          >
            <div class="sftp-state">
              <Folder
                :size="20"
                stroke-width="1.7"
              />
              <span>{{ labels.emptyFolder }}</span>
            </div>
          </div>
        </div>
        <template v-else>
          <div
            v-if="visibleRange.top"
            class="sftp-virtual-spacer"
            :style="{ '--sftp-virtual-spacer-block': `${visibleRange.top}px` }"
          />
          <template
            v-for="row in visibleRows"
            :key="row.key"
          >
            <div
              v-if="row.type === 'parent'"
              tabindex="0"
              class="sftp-row sftp-parent-row is-dir"
              :class="{
                'sftp-row-drop-target':
                  dropTargetPath === row.entry.path || moveDropTargetPath === row.entry.path,
              }"
              role="row"
              :data-path="row.entry.path"
              :data-row-key="row.key"
              @dblclick="$emit('openParent')"
              @keydown.enter.prevent="$emit('openParent')"
            >
              <div role="cell">
                <span class="sftp-name-cell">
                  <component
                    :is="iconForSftpEntry(row.entry)"
                    :size="16"
                    stroke-width="1.8"
                    class="text-accent"
                  />
                  <span>..</span>
                </span>
              </div>
              <div role="cell">
                {{ labels.folder }}
              </div>
              <div role="cell">
                -
              </div>
              <div
                role="cell"
                class="sftp-compact-hidden"
              >
                -
              </div>
            </div>
            <div
              v-else-if="row.type === 'creating'"
              class="sftp-row is-selected is-editing sftp-row-selected sftp-row-editing"
              :class="{ 'is-dir': creatingFolder }"
              role="row"
              :data-row-key="row.key"
            >
              <div role="cell">
                <span class="sftp-name-cell">
                  <component
                    :is="creatingFolder ? Folder : File"
                    :size="16"
                    stroke-width="1.8"
                    :class="{ 'text-accent': creatingFolder }"
                  />
                  <input
                    :ref="setInlineEditInputRef"
                    :value="inlineEdit.value"
                    class="sftp-inline-name-input"
                    :disabled="inlineEdit.committing"
                    :aria-label="creatingFolder ? labels.newFolder : labels.newFile"
                    @click.stop
                    @dblclick.stop
                    @input="$emit('updateInlineEditValue', $event.target.value)"
                    @keydown.enter.prevent="commitInlineEdit"
                    @keydown.esc.prevent="cancelInlineEdit"
                    @blur="commitInlineEdit"
                  >
                </span>
              </div>
              <div role="cell">
                {{ creatingFolder ? labels.folder : labels.file }}
              </div>
              <div role="cell">
                -
              </div>
              <div
                role="cell"
                class="sftp-compact-hidden"
              >
                -
              </div>
            </div>
            <div
              v-else
              tabindex="0"
              class="sftp-row"
              :class="{
                'sftp-row-selected': selectedNames.has(row.entry.name),
                'is-dir': row.entry.kind === 'dir',
                'sftp-move-draggable': !isEditingEntry(row.entry),
                'sftp-row-drop-target':
                  dropTargetPath === row.entry.path || moveDropTargetPath === row.entry.path,
                'sftp-row-editing': isEditingEntry(row.entry),
              }"
              role="row"
              :data-path="row.entry.path"
              :data-change="row.entry.animation || null"
              :data-row-key="row.key"
              @click="$emit('selectEntry', row.entry, $event)"
              @dblclick="$emit('openEntry', row.entry)"
              @keydown.enter.prevent="$emit('openEntry', row.entry)"
              @keydown.f2.prevent="$emit('startRenameEntry', row.entry)"
            >
              <div role="cell">
                <span class="sftp-name-cell">
                  <component
                    :is="iconForSftpEntry(row.entry)"
                    :size="16"
                    stroke-width="1.8"
                    :class="{
                      'text-accent': row.entry.kind === 'dir' || row.entry.kind === 'symlink',
                    }"
                  />
                  <input
                    v-if="isEditingEntry(row.entry)"
                    :ref="setInlineEditInputRef"
                    :value="inlineEdit.value"
                    class="sftp-inline-name-input"
                    :disabled="inlineEdit.committing"
                    :aria-label="labels.rename"
                    @click.stop
                    @dblclick.stop
                    @input="$emit('updateInlineEditValue', $event.target.value)"
                    @keydown.enter.prevent="commitInlineEdit"
                    @keydown.esc.prevent="cancelInlineEdit"
                    @blur="commitInlineEdit"
                  >
                  <span v-else>{{ row.entry.name }}</span>
                </span>
              </div>
              <div role="cell">
                {{ fileTypeLabel(row.entry) }}
              </div>
              <div role="cell">
                {{ row.entry.kind === "dir" ? "-" : formatBytes(row.entry.size) }}
              </div>
              <div
                role="cell"
                class="sftp-compact-hidden"
              >
                {{ formatModified(row.entry.modified) }}
              </div>
            </div>
          </template>
          <div
            v-if="visibleRange.bottom"
            class="sftp-virtual-spacer"
            :style="{ '--sftp-virtual-spacer-block': `${visibleRange.bottom}px` }"
          />
        </template>
      </div>
    </div>
  </main>
</template>
