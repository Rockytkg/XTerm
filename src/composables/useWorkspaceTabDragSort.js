import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useSortableList } from "./useSortableList";

// 工作区会话标签的横向拖拽排序：排序骨架复用 useSortableList，这里只保留
// 标签特有的 Sortable 参数（横向布局、状态类名、单标签仍可拖、关闭按钮过滤）
// 与生命周期绑定；拖拽后的选中抑制沿用 composable 的 180ms 点击抑制。
export function useWorkspaceTabDragSort({ getTabIds, onReorder }) {
  const listRef = ref(null);
  const {
    dragging,
    isClickSuppressed,
    sortableCleanup,
    createSortable,
    destroySortable,
    syncSortableOrder,
  } = useSortableList({
    listRef,
    ids: getTabIds,
    draggable: ".workspace-session-tab-item",
    filter: ".tab-close-btn",
    delay: 170,
    delayOnTouchOnly: false,
    direction: "horizontal",
    scrollSensitivity: 42,
    scrollSpeed: 12,
    swapThreshold: 0.58,
    minItems: 1,
    classNames: {
      chosen: "ui-session-tab-sortable-chosen",
      ghost: "ui-session-tab-sortable-ghost",
      drag: "ui-session-tab-sortable-drag",
      fallback: "ui-session-tab-sortable-fallback",
    },
    onReorder,
  });

  onMounted(() => {
    nextTick(createSortable);
    sortableCleanup.bindReleaseCleanup();
  });

  onBeforeUnmount(() => {
    destroySortable();
    sortableCleanup.unbindReleaseCleanup();
  });

  return {
    dragging,
    isSelectionSuppressed: isClickSuppressed,
    listRef,
    syncSortableOrder,
  };
}
