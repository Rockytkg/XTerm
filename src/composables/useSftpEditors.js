import { computed, onBeforeUnmount, ref } from "vue";
import { readRemoteSftpFile, statRemoteSftpFile, writeRemoteSftpFile } from "../services/sftp";

function safeMessage(error) {
  if (typeof error === "string") return error;
  if (error?.message) return error.message;
  return String(error || "Unknown error");
}

function tabNameFromPath(path) {
  const normalized = String(path || "").replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? normalized.slice(index + 1) || normalized : normalized;
}

export function useSftpEditors({
  props,
  t,
  refreshCurrentDirectoryIncremental = () => {},
  requestDirtyAction = async () => "cancel",
  requestOverwriteAction = async () => false,
}) {
  const editorTabs = ref([]);
  const activeEditorPath = ref("");
  let disposed = false;

  const activeEditorTab = computed(
    () => editorTabs.value.find((tab) => tab.path === activeEditorPath.value) || null,
  );

  function patchTab(path, patch) {
    editorTabs.value = editorTabs.value.map((tab) =>
      tab.path === path ? { ...tab, ...patch } : tab,
    );
  }

  function connectionIds() {
    return {
      connectionId: props.connection?.id || "",
      sessionId: props.sessionId || "",
    };
  }

  function isStaleSession(session) {
    return (
      disposed ||
      !session.connectionId ||
      !session.sessionId ||
      props.connection?.id !== session.connectionId ||
      props.sessionId !== session.sessionId
    );
  }

  async function openEditor(entry) {
    if (!entry || entry.kind === "dir") return;

    const existing = editorTabs.value.find((tab) => tab.path === entry.path);
    if (existing) {
      activeEditorPath.value = existing.path;
      return;
    }

    const session = connectionIds();
    if (isStaleSession(session)) return;

    const tab = {
      path: entry.path,
      name: entry.name || tabNameFromPath(entry.path),
      content: "",
      savedContent: "",
      modified: entry.modified || null,
      loading: true,
      saving: false,
      error: "",
    };
    editorTabs.value = [...editorTabs.value, tab];
    activeEditorPath.value = tab.path;

    try {
      const [content, stat] = await Promise.all([
        readRemoteSftpFile(session.connectionId, session.sessionId, entry.path),
        statRemoteSftpFile(session.connectionId, session.sessionId, entry.path),
      ]);
      if (isStaleSession(session)) return;
      patchTab(entry.path, {
        content,
        savedContent: content,
        modified: stat?.modified || null,
        loading: false,
        error: "",
      });
    } catch (error) {
      if (isStaleSession(session)) return;
      patchTab(entry.path, {
        loading: false,
        error: `${t("sftp.editor.openFailed")}: ${safeMessage(error)}`,
      });
    }
  }

  async function saveEditor(tab = activeEditorTab.value) {
    if (!tab || tab.loading || tab.saving) return false;

    const session = connectionIds();
    if (isStaleSession(session)) return false;
    patchTab(tab.path, { saving: true, error: "" });

    try {
      const latest = await statRemoteSftpFile(session.connectionId, session.sessionId, tab.path);
      if (isStaleSession(session)) return false;
      if (
        tab.modified &&
        latest?.modified &&
        latest.modified !== tab.modified &&
        !(await requestOverwriteAction({ kind: "overwrite", tab }))
      ) {
        if (isStaleSession(session)) return false;
        patchTab(tab.path, { saving: false });
        return false;
      }

      const saved = await writeRemoteSftpFile(
        session.connectionId,
        session.sessionId,
        tab.path,
        tab.content,
      );
      if (isStaleSession(session)) return false;
      await refreshCurrentDirectoryIncremental();
      if (isStaleSession(session)) return false;
      patchTab(tab.path, {
        savedContent: tab.content,
        modified: saved?.modified || latest?.modified || tab.modified,
        saving: false,
        error: "",
      });
      return true;
    } catch (error) {
      if (isStaleSession(session)) return false;
      patchTab(tab.path, {
        saving: false,
        error: `${t("sftp.editor.saveFailed")}: ${safeMessage(error)}`,
      });
      return false;
    }
  }

  async function closeEditor(tab) {
    if (!tab) return false;

    if (tab.content !== tab.savedContent) {
      const action = await requestDirtyAction({ kind: "close", tab });
      if (action === "save") {
        const saved = await saveEditor(tab);
        if (!saved) return false;
      } else if (action !== "discard") {
        return false;
      }
    }

    const index = editorTabs.value.findIndex((item) => item.path === tab.path);
    editorTabs.value = editorTabs.value.filter((item) => item.path !== tab.path);
    if (activeEditorPath.value === tab.path) {
      const next = editorTabs.value[Math.max(0, index - 1)] || editorTabs.value[0] || null;
      activeEditorPath.value = next?.path || "";
    }
    return true;
  }

  function updateEditorContent(path, content) {
    patchTab(path, { content });
  }

  function discardEditorChanges(tab = activeEditorTab.value) {
    if (!tab) return;
    patchTab(tab.path, {
      content: tab.savedContent,
      error: "",
    });
  }

  onBeforeUnmount(() => {
    disposed = true;
  });

  return {
    activeEditorPath,
    activeEditorTab,
    closeEditor,
    discardEditorChanges,
    editorTabs,
    openEditor,
    saveEditor,
    updateEditorContent,
  };
}
