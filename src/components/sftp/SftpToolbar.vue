<script setup>
import { Download, FolderSync, RefreshCw, Upload } from "@lucide/vue";

defineProps({
  canDownload: { type: Boolean, default: false },
  downloadLabel: { type: String, required: true },
  endpointLabel: { type: String, default: "-" },
  refreshLabel: { type: String, required: true },
  title: { type: String, required: true },
  uploadLabel: { type: String, required: true },
});

defineEmits(["download", "refresh", "upload"]);
</script>

<template>
  <header class="sftp-toolbar">
    <div class="sftp-title">
      <span class="sftp-title-icon">
        <FolderSync
          :size="16"
          stroke-width="1.9"
        />
      </span>
      <div class="min-w-0 grid gap-[1px]">
        <strong class="text-[0.9286em] leading-[1.25]">{{ title }}</strong>
        <span class="truncate text-[0.75em] leading-[1.25] text-text-tertiary">{{
          endpointLabel
        }}</span>
      </div>
    </div>

    <div class="sftp-toolbar-actions">
      <button
        type="button"
        class="sftp-button"
        :disabled="!canDownload"
        @click="$emit('download')"
      >
        <Download
          :size="14"
          stroke-width="1.9"
        />
        <span>{{ downloadLabel }}</span>
      </button>
      <button
        type="button"
        class="sftp-button sftp-button-primary"
        @click="$emit('upload')"
      >
        <Upload
          :size="14"
          stroke-width="1.9"
        />
        <span>{{ uploadLabel }}</span>
      </button>
      <button
        type="button"
        class="sftp-icon-button"
        :title="refreshLabel"
        @click="$emit('refresh')"
      >
        <RefreshCw
          :size="14"
          stroke-width="1.9"
        />
      </button>
    </div>
  </header>
</template>
