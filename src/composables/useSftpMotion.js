import { nextTick, onBeforeUnmount, watch } from "vue";
import { gsap } from "gsap";
import { motionEnabled } from "../utils/motion";

const ROW_ENTER = {
  autoAlpha: 0,
  y: 4,
  scale: 0.998,
};

function canAnimate() {
  return motionEnabled();
}

function visibleRows(tableBody) {
  return Array.from(tableBody?.querySelectorAll?.(".sftp-row:not(.sftp-skeleton-row)") ?? []);
}

function skeletonParts(tableBody) {
  return Array.from(tableBody?.querySelectorAll?.(".sftp-skeleton-icon, .sftp-skeleton-bar") ?? []);
}

function queueItems(queueList) {
  return Array.from(queueList?.querySelectorAll?.(".sftp-queue-item") ?? []);
}

function rowKey(row) {
  return row?.dataset?.path || row?.dataset?.rowKey || "";
}

function clearRows(rows) {
  if (!rows.length) return;
  gsap.killTweensOf(rows);
  gsap.set(rows, { clearProps: "all" });
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
  let skeletonTween = null;
  let dropTween = null;

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

    if (!canAnimate() || !enteringRows.length) {
      clearRows(enteringRows);
      return;
    }

    gsap.fromTo(enteringRows, ROW_ENTER, {
      autoAlpha: 1,
      y: 0,
      scale: 1,
      duration: 0.24,
      ease: "power3.out",
      stagger: { each: 0.012, from: "start" },
      clearProps: "opacity,visibility,transform",
    });
  }

  function pulseChangedRows() {
    const rows = visibleRows(tableBodyRef.value).filter((row) => row.dataset.change);
    if (!rows.length || !canAnimate()) return;

    gsap.fromTo(
      rows,
      { x: 0 },
      {
        x: 0,
        duration: 0.28,
        ease: "power2.out",
        keyframes: [{ x: -1 }, { x: 1 }, { x: 0 }],
        clearProps: "transform",
      },
    );
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

    if (!canAnimate() || !entering.length) {
      clearRows(entering);
      return;
    }

    gsap.fromTo(
      entering,
      { autoAlpha: 0, y: 6, scale: 0.995 },
      {
        autoAlpha: 1,
        y: 0,
        scale: 1,
        duration: 0.22,
        ease: "power3.out",
        stagger: 0.018,
        clearProps: "opacity,visibility,transform",
      },
    );
  }

  function animateDragState(active) {
    const table = tableBodyRef.value;
    const browser = table?.closest?.(".sftp-browser");
    if (!table) return;
    dropTween?.kill();
    if (!canAnimate()) {
      if (browser) {
        gsap.set(
          browser,
          active
            ? { "--sftp-drop-opacity": 1 }
            : { clearProps: "--sftp-drop-opacity,--sftp-drop-ring" },
        );
      }
      return;
    }
    if (!browser) return;
    dropTween = gsap.to(browser, {
      "--sftp-drop-opacity": active ? 1 : 0,
      "--sftp-drop-ring": active ? "4px" : "0px",
      duration: active ? 0.18 : 0.16,
      ease: active ? "power3.out" : "power2.inOut",
      onComplete: () => {
        if (!active) {
          gsap.set(browser, {
            clearProps: "--sftp-drop-opacity,--sftp-drop-ring",
          });
        }
      },
    });
  }

  function stopSkeletonAnimation({ clear = true } = {}) {
    if (Array.isArray(skeletonTween)) {
      for (const tween of skeletonTween) tween.kill();
    } else {
      skeletonTween?.kill();
    }
    skeletonTween = null;
    const parts = skeletonParts(tableBodyRef.value);
    if (clear && parts.length) {
      gsap.set(parts, { clearProps: "--sftp-skeleton-shimmer" });
    }
  }

  function animateSkeleton() {
    const parts = skeletonParts(tableBodyRef.value);
    if (!loading.value || !parts.length) {
      stopSkeletonAnimation();
      return;
    }
    if (!canAnimate()) {
      stopSkeletonAnimation({ clear: false });
      return;
    }
    stopSkeletonAnimation({ clear: false });
    skeletonTween = parts.map((part, index) => {
      gsap.set(part, { "--sftp-skeleton-shimmer": "-160px" });
      return gsap.to(part, {
        "--sftp-skeleton-shimmer": "360px",
        duration: 2.15,
        delay: (index % 4) * 0.11,
        ease: "none",
        repeat: -1,
        repeatDelay: 0.28,
      });
    });
  }

  watch(
    [filteredRemoteFiles, loading],
    () => {
      nextTick(() => {
        animateSkeleton();
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
    dropTween?.kill();
    clearRows(visibleRows(tableBodyRef.value));
    stopSkeletonAnimation();
    clearRows(queueItems(queueListRef.value));
  });
}
