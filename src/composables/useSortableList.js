import { nextTick, ref, watch } from "vue";
import Sortable from "sortablejs";
import { sortableMotion } from "../utils/motion";
import { createSortableCleanup } from "../utils/sortableCleanup";

const ORDER_SEPARATOR = "\u001f";
const SORTABLE_STATE_CLASSES = [
  "session-card-sortable-chosen",
  "session-card-sortable-ghost",
  "session-card-sortable-drag",
  "session-card-sortable-fallback",
];

function sameOrder(a, b) {
  return a.length === b.length && a.every((id, index) => id === b[index]);
}

// 卡片列表拖拽排序的共享骨架：Sortable 实例生命周期、DOM 顺序与数据顺序同步、
// 拖拽释放后的状态类清理。各视图的差异（id 来源、拖拽选项、持久化回调）
// 全部经参数注入；onReorder 抛错时由 onReorderError 统一兜底提示。
export function useSortableList({
  listRef,
  ids,
  draggable,
  filter,
  delay,
  delayOnTouchOnly,
  enabled = () => true,
  onDragEnd,
  onReorder,
  onReorderError,
}) {
  const dragging = ref(false);
  let sortable = null;
  const sortableCleanup = createSortableCleanup({
    classNames: SORTABLE_STATE_CLASSES,
    onReset: () => {
      dragging.value = false;
    },
  });

  function syncSortableOrder() {
    if (!sortable || dragging.value) return;
    const orderedIds = ids();
    const domOrder = sortable.toArray().filter(Boolean);
    sortable.option("disabled", orderedIds.length < 2);
    if (!sameOrder(domOrder, orderedIds)) {
      sortable.sort(orderedIds, false);
    }
  }

  function createSortable() {
    const list = listRef.value;
    if (!list || sortable || !enabled()) return;

    sortable = Sortable.create(list, {
      ...sortableMotion,
      draggable,
      dataIdAttr: "data-id",
      delay,
      delayOnTouchOnly,
      touchStartThreshold: 4,
      fallbackClass: "session-card-sortable-fallback",
      fallbackTolerance: 5,
      forceFallback: true,
      fallbackOnBody: true,
      scroll: true,
      bubbleScroll: false,
      scrollSensitivity: 48,
      scrollSpeed: 14,
      swapThreshold: 0.62,
      ghostClass: "session-card-sortable-ghost",
      chosenClass: "session-card-sortable-chosen",
      dragClass: "session-card-sortable-drag",
      filter,
      preventOnFilter: false,
      onStart() {
        dragging.value = true;
      },
      async onEnd() {
        const nextOrder = sortable.toArray().filter(Boolean);
        sortableCleanup.resetSortableState();
        onDragEnd?.();

        if (!sameOrder(nextOrder, ids())) {
          try {
            await onReorder(nextOrder);
          } catch (error) {
            onReorderError?.(error);
          }
        }

        nextTick(syncSortableOrder);
      },
      onUnchoose() {
        sortableCleanup.resetSortableState();
      },
    });

    syncSortableOrder();
  }

  function destroySortable() {
    sortable?.destroy();
    sortable = null;
    dragging.value = false;
  }

  watch(
    () => ids().join(ORDER_SEPARATOR),
    () => {
      nextTick(syncSortableOrder);
    },
  );

  return {
    dragging,
    sortableCleanup,
    createSortable,
    destroySortable,
  };
}
