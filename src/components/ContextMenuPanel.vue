<script setup>
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { contextMenuIconFor } from "./contextMenuIcons";
import { isContextMenuDangerItem } from "../utils/contextMenu";
import "../styles/context-menu.scss";

/**
 * 右键菜单面板：悬浮窗口菜单（FloatingContextMenuWindow）与 Wayland 下的
 * DOM 降级菜单（DomContextMenu）共用同一份渲染。定位、显隐、动作分发留在
 * 各自的宿主里；这里只管条目渲染与焦点。
 */
defineProps({
  items: { type: Array, required: true },
  theme: { type: String, default: "light" },
});

const emit = defineEmits(["activate"]);

const menuRef = ref(null);
const { t } = useI18n();

function itemLabel(item) {
  return item?.labelKey ? t(item.labelKey) : item?.label || "";
}

function focus() {
  menuRef.value?.focus({ preventScroll: true });
}

function containsFocusedElement() {
  return !!menuRef.value?.contains(document.activeElement);
}

defineExpose({ focus, containsFocusedElement });
</script>

<template>
  <main
    ref="menuRef"
    class="floating-context-menu"
    :data-theme="theme"
    aria-label="Context menu"
    role="menu"
    tabindex="-1"
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
        :class="{ 'is-danger': isContextMenuDangerItem(item) }"
        :disabled="!item.enabled"
        :data-disabled="!item.enabled ? '' : undefined"
        role="menuitem"
        @click="emit('activate', item)"
      >
        <span
          class="ui-context-menu-icon"
          aria-hidden="true"
        >
          <component
            :is="contextMenuIconFor(item)"
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
