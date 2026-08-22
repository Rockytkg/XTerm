import { nextTick } from "vue";

const ENTER_DIRECTORY_DELAY_MS = 460;

export function normalizeRemotePath(path) {
  if (!path) return ".";
  const absolute = path.startsWith("/");
  const parts = [];
  for (const part of path.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (parts.length && parts[parts.length - 1] !== "..") {
        parts.pop();
      } else if (!absolute) {
        parts.push(part);
      }
      continue;
    }
    parts.push(part);
  }
  if (absolute) return parts.length ? `/${parts.join("/")}` : "/";
  return parts.length ? parts.join("/") : ".";
}

export function isSameOrChildPath(path, parent) {
  const normalizedPath = normalizeRemotePath(path);
  const normalizedParent = normalizeRemotePath(parent);
  if (normalizedPath === normalizedParent) return true;
  const prefix = normalizedParent.endsWith("/") ? normalizedParent : `${normalizedParent}/`;
  return normalizedPath.startsWith(prefix);
}

function browserFromPoint(x, y) {
  const element = document.elementFromPoint(x, y);
  const browser = element?.closest?.(".sftp-browser");
  if (!browser) return { element: null, browser: null };
  return { element, browser };
}

export function createSftpDragNavigator({
  remotePath,
  remoteParent,
  refreshRemote,
  setDropTargetPath,
}) {
  let enterDirectoryTimer = 0;
  let pendingEnterDirectory = "";
  let navigating = false;
  let queuedEnter = null;

  function clearEnterDirectoryTimer() {
    if (enterDirectoryTimer) {
      window.clearTimeout(enterDirectoryTimer);
      enterDirectoryTimer = 0;
    }
    pendingEnterDirectory = "";
    queuedEnter = null;
  }

  function reset() {
    clearEnterDirectoryTimer();
    setDropTargetPath("");
  }

  function directoryTargetFromPoint(x, y, options = {}) {
    const { element, browser } = browserFromPoint(x, y);
    if (!browser) return { insideBrowser: false, rowPath: "", directoryPath: "" };

    const row = element?.closest?.(".sftp-row.is-dir[data-path]");
    if (!row || !browser.contains(row)) {
      return { insideBrowser: true, rowPath: "", directoryPath: remotePath.value };
    }

    if (row.classList.contains("sftp-parent-row")) {
      return {
        insideBrowser: true,
        rowPath: row.dataset.path || "",
        directoryPath: remoteParent.value || row.dataset.path || remotePath.value,
      };
    }

    const rowPath = row.dataset.path || "";
    if (
      options.sourcePath &&
      normalizeRemotePath(rowPath) === normalizeRemotePath(options.sourcePath)
    ) {
      return { insideBrowser: true, rowPath: "", directoryPath: remotePath.value };
    }

    return { insideBrowser: true, rowPath, directoryPath: rowPath || remotePath.value };
  }

  async function enterDirectory(path, session) {
    if (!session?.active) return;
    if (navigating) {
      queuedEnter = { path, session };
      return;
    }
    navigating = true;
    try {
      const loaded = await refreshRemote(path, {
        pathLoading: true,
        preserveEntries: true,
        suppressError: true,
      });
      if (loaded && session.active) {
        session.destinationDirectory = remotePath.value;
        setDropTargetPath("");
        await nextTick();
      }
    } finally {
      navigating = false;
      const nextEnter = queuedEnter;
      queuedEnter = null;
      if (nextEnter?.session?.active) {
        scheduleEnterDirectory(nextEnter.path, nextEnter.session);
      }
    }
  }

  function scheduleEnterDirectory(path, session) {
    if (!session?.active || !path) return;
    const normalizedTarget = normalizeRemotePath(path);
    const normalizedCurrent = normalizeRemotePath(remotePath.value);
    if (normalizedTarget === normalizedCurrent) {
      clearEnterDirectoryTimer();
      return;
    }
    if (pendingEnterDirectory === normalizedTarget) return;

    clearEnterDirectoryTimer();
    pendingEnterDirectory = normalizedTarget;
    enterDirectoryTimer = window.setTimeout(() => {
      enterDirectoryTimer = 0;
      pendingEnterDirectory = "";
      enterDirectory(path, session);
    }, ENTER_DIRECTORY_DELAY_MS);
  }

  function updateFromPoint(x, y, session, options = {}) {
    if (!session?.active) return { insideBrowser: false, rowPath: "", directoryPath: "" };

    const target = directoryTargetFromPoint(x, y, options);
    if (!target.insideBrowser) {
      session.dropAllowed = false;
      setDropTargetPath("");
      clearEnterDirectoryTimer();
      return target;
    }

    session.dropAllowed = true;
    session.destinationDirectory = target.directoryPath || remotePath.value;
    setDropTargetPath(target.rowPath || "");

    if (target.rowPath && target.directoryPath) {
      scheduleEnterDirectory(target.directoryPath, session);
    } else {
      clearEnterDirectoryTimer();
    }

    return target;
  }

  return {
    reset,
    updateFromPoint,
  };
}
