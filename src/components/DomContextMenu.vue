<script setup>
import { computed, nextTick, ref, watch } from "vue";
import {
  activateDomContextMenuItem,
  dismissContextMenu,
  domContextMenuState,
} from "../services/contextMenu";
import ContextMenuPanel from "./ContextMenuPanel.vue";

const menuRef = ref(null);
let previousActiveElement = null;

const panelStyle = computed(() => ({
  "--dom-context-menu-x": `${domContextMenuState.x}px`,
  "--dom-context-menu-y": `${domContextMenuState.y}px`,
  "--context-menu-panel-width": `${domContextMenuState.width}px`,
  "--context-menu-max-height": `${domContextMenuState.maxHeight}px`,
}));

// 悬浮窗口菜单 focusable: false 不会抢焦点；DOM 菜单为了 Escape/键盘可用
// 需要获得焦点，关闭时若焦点仍停留在菜单内则归还给之前的元素。
watch(
  () => domContextMenuState.visible,
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
    <!-- 遮罩不拦截 contextmenu：右键菜单外区域时交给 document 监听器重新定位
         （与悬浮窗口“右键换位”行为一致）；面板自身阻止，避免菜单上右键抖动。 -->
    <div
      v-if="domContextMenuState.visible && domContextMenuState.items.length"
      class="dom-context-menu-overlay"
    >
      <ContextMenuPanel
        ref="menuRef"
        class="dom-context-menu-panel"
        :items="domContextMenuState.items"
        :theme="domContextMenuState.theme"
        :style="panelStyle"
        @contextmenu.stop.prevent
        @keydown.escape.stop.prevent="dismissContextMenu()"
        @activate="activateDomContextMenuItem($event.id)"
      />
    </div>
  </Teleport>
</template>
