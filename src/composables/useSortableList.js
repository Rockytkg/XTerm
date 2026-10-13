import { nextTick, ref, watch } from "vue";
import Sortable from "sortablejs";
import { getSortableMotion } from "../utils/motion";
import { createSortableCleanup } from "../utils/sortableCleanup";
import { sameOrder } from "../utils/listOrder";

const ORDER_SEPARATOR = "\u001f";
const CLICK_SUPPRESS_MS = 180;
const DEFAULT_CLASS_NAMES = {
  chosen: "session-card-sortable-chosen",
  ghost: "session-card-sortable-ghost",
  drag: "session-card-sortable-drag",
  fallback: "session-card-sortable-fallback",
};

// template ref 可能指向组件实例（如 reka-ui 的 TabsList），统一取回 HTMLElement。
function elementFromRef(templateRef) {
  const value = templateRef.value;
  if (!value) return null;
  if (value instanceof HTMLElement) return value;
  return value.$el instanceof HTMLElement ? value.$el : null;
}

// 列表拖拽排序的共享骨架：Sortable 实例生命周期、DOM 顺序与数据顺序同步、
// 拖拽释放后的状态类清理、拖拽后 180ms 内的误触点击抑制。各视图的差异
// （id 来源、拖拽选项、状态类名、enabled 开关、持久化回调）全部经参数注入；
// onReorder 抛错时由 onReorderError 统一兜底提示。
export function useSortableList({
  listRef,
  ids,
  draggable,
  filter,
  delay,
  delayOnTouchOnly,
  direction,
  scrollSensitivity = 48,
  scrollSpeed = 14,
  swapThreshold = 0.62,
  minItems = 2,
  classNames = DEFAULT_CLASS_NAMES,
  enabled = () => true,
  onStart,
  onDragEnd,
  onReorder,
  onReorderError,
}) {
  const dragging = ref(false);
  let sortable = null;
  let suppressClickTimer = 0;
  let suppressNextClick = false;
  const sortableCleanup = createSortableCleanup({
    classNames: [classNames.chosen, classNames.ghost, classNames.drag, classNames.fallback],
    onReset: () => {
      dragging.value = false;
    },
  });

  // 拖拽释放后 click 紧跟 pointerup 触发，180ms 窗口内视为拖拽误触吞掉；
  // onStart/onEnd 双重武装，长按超过窗口期的拖拽由 onEnd 重新计时。
  function suppressUpcomingClick() {
    suppressNextClick = true;
    if (suppressClickTimer) window.clearTimeout(suppressClickTimer);
    suppressClickTimer = window.setTimeout(() => {
      suppressNextClick = false;
      suppressClickTimer = 0;
    }, CLICK_SUPPRESS_MS);
  }

  function clearClickSuppression() {
    suppressNextClick = false;
    if (suppressClickTimer) {
      window.clearTimeout(suppressClickTimer);
      suppressClickTimer = 0;
    }
  }

  function isClickSuppressed() {
    return dragging.value || suppressNextClick;
  }

  function consumeSuppressedClick() {
    if (!isClickSuppressed()) return false;
    clearClickSuppression();
    return true;
  }

  function syncSortableOrder() {
    if (!sortable || dragging.value) return;
    const orderedIds = ids();
    const domOrder = sortable.toArray().filter(Boolean);
    sortable.option("disabled", orderedIds.length < minItems);
    if (!sameOrder(domOrder, orderedIds)) {
      sortable.sort(orderedIds, false);
    }
  }

  function createSortable() {
    const list = elementFromRef(listRef);
    if (!list || sortable || !enabled()) return;

    sortable = Sortable.create(list, {
      ...getSortableMotion(),
      draggable,
      dataIdAttr: "data-id",
      // sortablejs 用 `name in options` 判定是否套用默认值，direction 传
      // undefined 会顶掉默认的横竖自动检测，因此仅在显式指定时透传。
      ...(direction ? { direction } : {}),
      delay,
      delayOnTouchOnly,
      touchStartThreshold: 4,
      fallbackClass: classNames.fallback,
      fallbackTolerance: 5,
      forceFallback: true,
      fallbackOnBody: true,
      scroll: true,
      bubbleScroll: false,
      scrollSensitivity,
      scrollSpeed,
      swapThreshold,
      ghostClass: classNames.ghost,
      chosenClass: classNames.chosen,
      dragClass: classNames.drag,
      filter,
      preventOnFilter: false,
      onStart() {
        dragging.value = true;
        suppressUpcomingClick();
        onStart?.();
      },
      async onEnd() {
        const nextOrder = sortable.toArray().filter(Boolean);
        sortableCleanup.resetSortableState();
        suppressUpcomingClick();
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
    clearClickSuppression();
  }

  watch(
    () => ids().join(ORDER_SEPARATOR),
    () => {
      nextTick(syncSortableOrder);
    },
  );

  // enabled 动态开关：关闭即销毁；重新打开时 listRef 可能随 v-else 刚换回
  // 挂载，等 nextTick 让模板落完再创建。
  watch(enabled, async (isEnabled) => {
    if (!isEnabled) {
      destroySortable();
      return;
    }
    await nextTick();
    createSortable();
  });

  return {
    dragging,
    sortableCleanup,
    isClickSuppressed,
    consumeSuppressedClick,
    createSortable,
    destroySortable,
    syncSortableOrder,
  };
}
