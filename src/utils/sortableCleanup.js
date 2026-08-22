const DEFAULT_RELEASE_EVENTS = [
  "pointerup",
  "pointercancel",
  "mouseup",
  "touchend",
  "touchcancel",
  "dragend",
  "drop",
];

export function createSortableCleanup({ classNames, onReset, releaseDelayMs = 120 }) {
  const selector = classNames.map((className) => `.${className}`).join(",");
  let cleanupFrame = 0;
  let releaseCleanupTimer = 0;

  function clearSortableStateClasses() {
    cleanupFrame = 0;
    document
      .querySelectorAll(selector)
      .forEach((element) => element.classList.remove(...classNames));
  }

  function scheduleSortableStateCleanup() {
    if (cleanupFrame) window.cancelAnimationFrame(cleanupFrame);
    cleanupFrame = window.requestAnimationFrame(clearSortableStateClasses);
  }

  function resetSortableState() {
    onReset();
    scheduleSortableStateCleanup();
  }

  function scheduleReleaseCleanup() {
    if (releaseCleanupTimer) window.clearTimeout(releaseCleanupTimer);
    releaseCleanupTimer = window.setTimeout(() => {
      releaseCleanupTimer = 0;
      resetSortableState();
    }, releaseDelayMs);
  }

  function onVisibilityChange() {
    if (document.visibilityState === "hidden") resetSortableState();
  }

  function bindReleaseCleanup() {
    DEFAULT_RELEASE_EVENTS.forEach((eventName) => {
      window.addEventListener(eventName, scheduleReleaseCleanup, true);
    });
    window.addEventListener("blur", resetSortableState);
    document.addEventListener("visibilitychange", onVisibilityChange);
  }

  function unbindReleaseCleanup() {
    if (releaseCleanupTimer) window.clearTimeout(releaseCleanupTimer);
    if (cleanupFrame) window.cancelAnimationFrame(cleanupFrame);
    clearSortableStateClasses();
    DEFAULT_RELEASE_EVENTS.forEach((eventName) => {
      window.removeEventListener(eventName, scheduleReleaseCleanup, true);
    });
    window.removeEventListener("blur", resetSortableState);
    document.removeEventListener("visibilitychange", onVisibilityChange);
  }

  return {
    bindReleaseCleanup,
    clearSortableStateClasses,
    resetSortableState,
    scheduleReleaseCleanup,
    scheduleSortableStateCleanup,
    unbindReleaseCleanup,
  };
}
