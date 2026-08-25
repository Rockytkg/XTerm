<script setup>
import { computed, nextTick, ref, watch } from "vue";
import {
  activateContextMenuItem,
  contextMenuState,
  dismissContextMenu,
} from "../services/contextMenu";
import ContextMenuPanel from "./ContextMenuPanel.vue";

const menuRef = ref(null);
let previousActiveElement = null;

const panelStyle = computed(() => ({
  "--context-menu-x": `${contextMenuState.x}px`,
  "--context-menu-y": `${contextMenuState.y}px`,
  "--context-menu-panel-width": `${contextMenuState.width}px`,
  "--context-menu-max-height": `${contextMenuState.maxHeight}px`,
}));

// 菜单为了 Escape/键盘可用需要获得焦点，关闭时若焦点仍停留在菜单内
// 则归还给之前的元素。
watch(
  () => contextMenuState.visible,
  (visible) => {
    if (visible) {
      previousActiveElement =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      void nextTick(() => menuRef.value?.focus?.());
      return;
    }
    if (previousActiveElement && menuRef.value?.containsFocusedElement?.()) {
      previousActiveElement.focus({ preventScroll: true });
    }
    previousActiveElement = null;
  },
  { flush: "sync" },
);
</script>

<template>
  <Teleport to="body">
    <!-- 遮罩不拦截 contextmenu：右键菜单外区域时交给 document 监听器重新定位；
         面板自身阻止，避免菜单上右键抖动。 -->
    <div
      v-if="contextMenuState.visible && contextMenuState.items.length"
      class="context-menu-overlay"
    >
      <ContextMenuPanel
        ref="menuRef"
        class="context-menu-panel"
        data-context-menu-root
        :items="contextMenuState.items"
        :theme="contextMenuState.theme"
        :style="panelStyle"
        @contextmenu.stop.prevent
        @keydown.escape.stop.prevent="dismissContextMenu()"
        @activate="activateContextMenuItem($event.id)"
      />
    </div>
  </Teleport>
</template>
