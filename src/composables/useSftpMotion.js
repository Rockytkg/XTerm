import { nextTick, onBeforeUnmount, watch } from "vue";
import { motionEnabled } from "../utils/motion";

// WAAPI 原生补间，替代原 gsap 驱动。ENTER_EASING 为 gsap power3.out 的等价
// 贝塞尔；入场/抖动时长是 SFTP 列表编排特有的功能时长，不属于交互反馈 token 档位。
const ENTER_EASING = "cubic-bezier(0.215, 0.61, 0.355, 1)";

const ROW_ENTER = {
  keyframes: [
    { opacity: 0, transform: "translateY(4px) scale(0.998)" },
    { opacity: 1, transform: "translateY(0) scale(1)" },
  ],
  duration: 240,
  stagger: 12,
};

const TRANSFER_ENTER = {
  keyframes: [
    { opacity: 0, transform: "translateY(6px) scale(0.995)" },
    { opacity: 1, transform: "translateY(0) scale(1)" },
  ],
  duration: 220,
  stagger: 18,
};

const ROW_PULSE = {
  keyframes: [
    { transform: "translateX(0)" },
    { transform: "translateX(-1px)" },
    { transform: "translateX(1px)" },
    { transform: "translateX(0)" },
  ],
  duration: 280,
  easing: "cubic-bezier(0.25, 0.46, 0.45, 0.94)",
};

function canAnimate() {
  return motionEnabled();
}

function visibleRows(tableBody) {
  return Array.from(tableBody?.querySelectorAll?.(".sftp-row:not(.sftp-skeleton-row)") ?? []);
}

function queueItems(queueList) {
  return Array.from(queueList?.querySelectorAll?.(".sftp-queue-item") ?? []);
}

function rowKey(row) {
  return row?.dataset?.path || row?.dataset?.rowKey || "";
}

export function useSftpMotion({
  tableBodyRef,
  queueListRef,
  filteredRemoteFiles,
  loading,
  dragActive,
  moveDragActive,
  transfers,
}) {
  let previousRowKeys = new Set();
  let previousTransferIds = new Set();
  // 在途动画集合：元素重渲染/组件卸载时统一取消；不设 fill，结束自动还原。
  const activeAnimations = new Set();

  function play(el, keyframes, options) {
    const animation = el.animate(keyframes, options);
    activeAnimations.add(animation);
    const release = () => activeAnimations.delete(animation);
    animation.addEventListener("finish", release);
    animation.addEventListener("cancel", release);
    return animation;
  }

  function animateEntering(elements, { keyframes, duration, stagger }) {
    elements.forEach((el, index) => {
      play(el, keyframes, {
        duration,
        delay: index * stagger,
        easing: ENTER_EASING,
      });
    });
  }

  function animateRows() {
    const rows = visibleRows(tableBodyRef.value);
    if (!rows.length) {
      previousRowKeys = new Set();
      return;
    }

    const nextKeys = new Set(rows.map((row) => rowKey(row)).filter(Boolean));
    const enteringRows = rows.filter((row) => {
      const key = rowKey(row);
      return key && !previousRowKeys.has(key);
    });
    previousRowKeys = nextKeys;

    if (!canAnimate() || !enteringRows.length) return;
    animateEntering(enteringRows, ROW_ENTER);
  }

  function pulseChangedRows() {
    const rows = visibleRows(tableBodyRef.value).filter((row) => row.dataset.change);
    if (!rows.length || !canAnimate()) return;

    for (const row of rows) {
      play(row, ROW_PULSE.keyframes, { duration: ROW_PULSE.duration, easing: ROW_PULSE.easing });
    }
  }

  function animateTransferItems() {
    const list = queueListRef.value;
    if (!list) {
      previousTransferIds = new Set();
      return;
    }

    const items = queueItems(list);
    const ids = (transfers.value || []).map((item) => String(item.id));
    const nextIds = new Set(ids);
    const entering = items.filter((item) => {
      const key = item.dataset.transferId || "";
      return key && !previousTransferIds.has(key);
    });
    previousTransferIds = nextIds;

    if (!canAnimate() || !entering.length) return;
    animateEntering(entering, TRANSFER_ENTER);
  }

  // 拖放高亮改由 class + CSS transition 驱动（见 styles/sftp.scss 的
  // .sftp-browser-shell::after）；data-motion="off" 时全局规则把过渡归零。
  function animateDragState(active) {
    const shell = tableBodyRef.value?.closest?.(".sftp-browser-shell");
    shell?.classList.toggle("sftp-drop-active", active);
  }

  watch(
    [filteredRemoteFiles, loading],
    () => {
      nextTick(() => {
        animateRows();
        pulseChangedRows();
      });
    },
    { flush: "post" },
  );

  // animateTransferItems only cares about newly added ids; watching the
  // length avoids deep-traversing the array on every RAF progress update
  watch(
    () => transfers.value.length,
    () => {
      nextTick(animateTransferItems);
    },
    { flush: "post" },
  );

  watch([dragActive, moveDragActive], ([dropActive, moveActive]) => {
    animateDragState(Boolean(dropActive || moveActive));
  });

  onBeforeUnmount(() => {
    for (const animation of activeAnimations) animation.cancel();
    activeAnimations.clear();
  });
}
