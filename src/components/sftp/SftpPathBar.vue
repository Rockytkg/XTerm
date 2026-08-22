<script setup>
import { ChevronRight, ChevronUp, LoaderCircle, Search } from "@lucide/vue";

defineProps({
  editablePath: { type: String, default: "" },
  editingPath: { type: Boolean, default: false },
  editPathLabel: { type: String, required: true },
  pathCrumbs: { type: Array, default: () => [] },
  pathLoading: { type: Boolean, default: false },
  remoteParent: { type: String, default: "" },
  remotePathLabel: { type: String, required: true },
  remoteQuery: { type: String, default: "" },
  searchLabel: { type: String, required: true },
  setPathInputRef: { type: Function, default: () => {} },
});

defineEmits([
  "cancelPathEdit",
  "editPath",
  "navigate",
  "navigateParent",
  "submitPathEdit",
  "update:editablePath",
  "update:remoteQuery",
]);
</script>

<template>
  <section class="sftp-pathbar">
    <button
      type="button"
      class="sftp-icon-button"
      :disabled="!remoteParent"
      :title="remotePathLabel"
      @click="$emit('navigateParent')"
    >
      <ChevronUp
        :size="15"
        stroke-width="2"
      />
    </button>
    <div
      class="sftp-path-shell"
      :class="{ 'is-editing': editingPath }"
      @click="$emit('editPath')"
    >
      <form
        v-if="editingPath"
        class="sftp-path-edit"
        @submit.prevent="$emit('submitPathEdit')"
      >
        <input
          :ref="setPathInputRef"
          :value="editablePath"
          class="sftp-path-input"
          :aria-label="remotePathLabel"
          @input="$emit('update:editablePath', $event.target.value)"
          @click.stop
          @keyup.esc="$emit('cancelPathEdit')"
          @blur="$emit('submitPathEdit')"
        >
      </form>
      <nav
        v-else
        class="sftp-crumbs"
        :aria-label="remotePathLabel"
        @dblclick="$emit('editPath')"
      >
        <template
          v-for="(crumb, index) in pathCrumbs"
          :key="`${crumb.path}-${index}`"
        >
          <button
            type="button"
            class="sftp-crumb"
            @click.stop="$emit('navigate', crumb.path)"
          >
            {{ crumb.label }}
          </button>
          <ChevronRight
            v-if="index < pathCrumbs.length - 1"
            :size="13"
            stroke-width="1.8"
          />
        </template>
        <LoaderCircle
          v-if="pathLoading"
          :size="14"
          stroke-width="1.9"
          class="animate-spin"
        />
      </nav>
      <button
        v-if="!editingPath"
        type="button"
        class="sftp-path-text-button"
        @click.stop="$emit('editPath')"
      >
        {{ editPathLabel }}
      </button>
    </div>
    <div class="sftp-search">
      <Search
        :size="14"
        stroke-width="1.8"
      />
      <input
        :value="remoteQuery"
        class="ui-fill-block min-w-0 ui-fill-inline border-0 bg-transparent text-[0.7857em] text-text-primary outline-none"
        :placeholder="searchLabel"
        @input="$emit('update:remoteQuery', $event.target.value)"
      >
    </div>
  </section>
</template>
