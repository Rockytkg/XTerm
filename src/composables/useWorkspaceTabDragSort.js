import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import Sortable from "sortablejs";
import { createSortableCleanup } from "../utils/sortableCleanup";
import { sortableMotion } from "../utils/motion";

const SELECTION_SUPPRESS_MS = 180;
const TAB_ORDER_SEPARATOR = "\u001f";
const SORTABLE_STATE_CLASSES = [
  "ui-session-tab-sortable-chosen",
  "ui-session-tab-sortable-ghost",
  "ui-session-tab-sortable-drag",
  "ui-session-tab-sortable-fallback",
];

function ordersMatch(a, b) {
  return a.length === b.length && a.every((id, index) => id === b[index]);
}

function elementFromRef(templateRef) {
  const value = templateRef.value;
  if (!value) return null;
  if (value instanceof HTMLElement) return value;
  return value.$el instanceof HTMLElement ? value.$el : null;
}

export function useWorkspaceTabDragSort({ getTabIds, onReorder }) {
  const listRef = ref(null);
  const dragging = ref(false);

  let sortable = null;
  let selectionSuppressTimer = 0;
  let suppressSelection = false;
  const sortableCleanup = createSortableCleanup({
    classNames: SORTABLE_STATE_CLASSES,
    onReset: () => {
      dragging.value = false;
    },
  });

  function currentTabIds() {
    return getTabIds();
  }

  function isSelectionSuppressed() {
    return dragging.value || suppressSelection;
  }

  function suppressUpcomingSelection() {
    suppressSelection = true;
    if (selectionSuppressTimer) window.clearTimeout(selectionSuppressTimer);
    selectionSuppressTimer = window.setTimeout(() => {
      suppressSelection = false;
      selectionSuppressTimer = 0;
    }, SELECTION_SUPPRESS_MS);
  }

  function syncSortableOrder() {
    if (!sortable || dragging.value) return;
    const ids = currentTabIds();
    const domOrder = sortable.toArray().filter(Boolean);
    sortable.option("disabled", ids.length < 1);
    if (!ordersMatch(domOrder, ids)) {
      sortable.sort(ids, false);
    }
  }

  function createSortable() {
    const list = elementFromRef(listRef);
    if (!list || sortable) return;

    sortable = Sortable.create(list, {
      ...sortableMotion,
      draggable: ".workspace-session-tab-item",
      dataIdAttr: "data-id",
      direction: "horizontal",
      delay: 170,
      delayOnTouchOnly: false,
      touchStartThreshold: 4,
      fallbackClass: "ui-session-tab-sortable-fallback",
      fallbackTolerance: 5,
      forceFallback: true,
      fallbackOnBody: true,
      scroll: true,
      bubbleScroll: false,
      scrollSensitivity: 42,
      scrollSpeed: 12,
      swapThreshold: 0.58,
      ghostClass: "ui-session-tab-sortable-ghost",
      chosenClass: "ui-session-tab-sortable-chosen",
      dragClass: "ui-session-tab-sortable-drag",
      filter: ".tab-close-btn",
      preventOnFilter: false,
      onStart() {
        dragging.value = true;
        suppressUpcomingSelection();
      },
      onEnd() {
        const nextOrder = sortable.toArray().filter(Boolean);
        sortableCleanup.resetSortableState();
        suppressUpcomingSelection();

        if (!ordersMatch(nextOrder, currentTabIds())) {
          onReorder(nextOrder);
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
    sortableCleanup.resetSortableState();
  }

  watch(
    () => currentTabIds().join(TAB_ORDER_SEPARATOR),
    () => {
      nextTick(syncSortableOrder);
    },
  );

  onMounted(() => {
    nextTick(createSortable);
    sortableCleanup.bindReleaseCleanup();
  });

  onBeforeUnmount(() => {
    destroySortable();
    if (selectionSuppressTimer) window.clearTimeout(selectionSuppressTimer);
    sortableCleanup.unbindReleaseCleanup();
  });

  return {
    dragging,
    isSelectionSuppressed,
    listRef,
    syncSortableOrder,
  };
}
