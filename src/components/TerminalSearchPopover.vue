<script setup>
import { onBeforeUnmount } from "vue";
import { useI18n } from "vue-i18n";
import { PopoverAnchor, PopoverContent, PopoverRoot } from "reka-ui";
import { ChevronDown, ChevronUp, Search, X } from "@lucide/vue";
import { createDebounced } from "../utils/schedulers";

const props = defineProps({
  term: { type: String, default: "" },
  resultLabel: { type: String, required: true },
  isEmpty: { type: Boolean, default: false },
});

const emit = defineEmits(["update:term", "run", "close"]);
const { t } = useI18n();
const debouncedRunSearch = createDebounced(() => run(false), 120);

function run(previous = false) {
  emit("run", { previous });
}

function runNow(previous = false) {
  debouncedRunSearch.cancel();
  run(previous);
}

function closeSearch() {
  debouncedRunSearch.cancel();
  emit("close");
}

function updateOpen(value) {
  if (!value) closeSearch();
}

function handleInput(event) {
  const value = event.target.value;
  emit("update:term", value);
  if (value.trim()) {
    debouncedRunSearch();
  } else {
    runNow(false);
  }
}

onBeforeUnmount(() => {
  debouncedRunSearch.cancel();
});
</script>

<template>
  <PopoverRoot
    :open="true"
    @update:open="updateOpen"
  >
    <PopoverAnchor as-child>
      <span class="terminal-search-anchor" />
    </PopoverAnchor>
    <PopoverContent
      as-child
      side="bottom"
      align="end"
      :side-offset="0"
      :collision-padding="10"
    >
      <form
        class="terminal-search-popover"
        role="search"
        @submit.prevent="run(false)"
      >
        <div class="terminal-search-input-wrap">
          <Search
            :size="14"
            stroke-width="1.9"
            class="terminal-search-icon"
          />
          <input
            :value="props.term"
            class="terminal-search-input"
            :placeholder="t('terminal.searchPlaceholder')"
            autofocus
            @input="handleInput"
            @keydown.enter.prevent.stop="runNow($event.shiftKey)"
          >
        </div>
        <span
          class="terminal-search-count"
          :class="{ 'terminal-search-count-empty': isEmpty }"
        >
          {{ resultLabel }}
        </span>
        <div
          class="terminal-search-actions"
          :aria-label="t('terminal.searchActions')"
        >
          <button
            type="button"
            class="terminal-search-button"
            :aria-label="t('terminal.searchPrevious')"
            :disabled="!props.term"
            @click="runNow(true)"
          >
            <ChevronUp
              :size="14"
              stroke-width="2"
            />
          </button>
          <button
            type="button"
            class="terminal-search-button"
            :aria-label="t('terminal.searchNext')"
            :disabled="!props.term"
            @click="runNow(false)"
          >
            <ChevronDown
              :size="14"
              stroke-width="2"
            />
          </button>
          <button
            type="button"
            class="terminal-search-button"
            :aria-label="t('terminal.searchClose')"
            @click="closeSearch"
          >
            <X
              :size="14"
              stroke-width="2"
            />
          </button>
        </div>
      </form>
    </PopoverContent>
  </PopoverRoot>
</template>
