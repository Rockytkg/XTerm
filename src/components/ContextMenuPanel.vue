<script setup>
import { nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { contextMenuIconFor } from "./contextMenuIcons";
import {
  isContextMenuActivatable,
  isContextMenuDangerItem,
  nextActivatableMenuIndex,
  typeaheadMenuIndex,
} from "../utils/contextMenu";
import "../styles/context-menu.scss";

/**
 * 右键菜单面板：条目渲染、悬停高亮与键盘导航（方向键/Home/End 移动、
 * Enter/Space 激活、首字符跳转），与原生菜单交互一致。
 * 定位、显隐、动作分发留在 services/contextMenu.js 与 ContextMenu.vue 里。
 */
const props = defineProps({
  items: { type: Array, required: true },
  theme: { type: String, default: "light" },
});

const emit = defineEmits(["activate"]);

const menuRef = ref(null);
const { t } = useI18n();

/** 当前高亮项索引（鼠标悬停或键盘导航）。-1 表示无高亮，与原生菜单一致。 */
const activeIndex = ref(-1);

// 菜单内容重建（重新打开/换一批条目）时清空高亮。
watch(
  () => props.items,
  () => {
    activeIndex.value = -1;
  },
);

function itemLabel(item) {
  return item?.labelKey ? t(item.labelKey) : item?.label || "";
}

function focus() {
  menuRef.value?.focus({ preventScroll: true });
}

function containsFocusedElement() {
  return !!menuRef.value?.contains(document.activeElement);
}

/** 指针移出菜单区域时清除悬停高亮（与原生菜单一致）；键盘导航不经指针事件，不受影响。 */
function clearActive() {
  activeIndex.value = -1;
}

function moveActive(index) {
  if (index < 0 || index === activeIndex.value) return;
  activeIndex.value = index;
  // 键盘导航到可视区外时滚动跟随；悬停时已可见则 scrollIntoView 无副作用。
  void nextTick(() => {
    menuRef.value
      ?.querySelector(`[data-menu-index="${index}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
}

function activateItem(item) {
  if (isContextMenuActivatable(item)) emit("activate", item);
}

function onKeydown(event) {
  const items = props.items;
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      moveActive(nextActivatableMenuIndex(items, activeIndex.value, 1));
      return;
    case "ArrowUp":
      event.preventDefault();
      moveActive(nextActivatableMenuIndex(items, activeIndex.value, -1));
      return;
    case "Home":
      event.preventDefault();
      moveActive(nextActivatableMenuIndex(items, -1, 1));
      return;
    case "End":
      event.preventDefault();
      moveActive(nextActivatableMenuIndex(items, items.length, -1));
      return;
    case "Enter":
    case " ":
      // 阻止按钮聚焦时的原生 click 触发，激活只走这里一次。
      event.preventDefault();
      activateItem(items[activeIndex.value]);
      return;
    default:
      // 首字符跳转（typeahead），与原生菜单的按键搜索一致。
      if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
        moveActive(typeaheadMenuIndex(items, activeIndex.value, event.key, itemLabel));
      }
  }
}

defineExpose({ focus, containsFocusedElement });
</script>

<template>
  <main
    ref="menuRef"
    class="context-menu-surface"
    :data-theme="theme"
    aria-label="Context menu"
    role="menu"
    tabindex="-1"
    @keydown="onKeydown"
    @pointerleave="clearActive"
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
        :data-highlighted="index === activeIndex && item.enabled ? '' : undefined"
        :data-menu-index="index"
        tabindex="-1"
        role="menuitem"
        @mousedown.prevent
        @pointerenter="item.enabled && moveActive(index)"
        @click="activateItem(item)"
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
