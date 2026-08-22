import { computed, onBeforeUnmount, ref } from "vue";

function createDirtyEditorDialogState() {
  return {
    count: 0,
    kind: "close",
    open: false,
    resolver: null,
    tab: null,
  };
}

function createConflictDialogState() {
  return {
    description: "",
    kind: "upload",
    name: "",
    open: false,
    resolver: null,
    title: "",
  };
}

export function useSftpDialogStates({ t }) {
  const dirtyEditorDialog = ref(createDirtyEditorDialogState());
  const conflictDialog = ref(createConflictDialogState());

  const editorConfirmTitle = computed(() => {
    if (dirtyEditorDialog.value.kind === "overwrite") {
      return t("sftp.editor.overwriteTitle", { name: dirtyEditorDialog.value.tab?.name || "" });
    }
    return dirtyEditorDialog.value.kind === "exit"
      ? t("sftp.editor.unsavedExitTitle", { count: dirtyEditorDialog.value.count })
      : t("sftp.editor.unsavedFileTitle", { name: dirtyEditorDialog.value.tab?.name || "" });
  });

  const editorConfirmDescription = computed(() => {
    if (dirtyEditorDialog.value.kind === "overwrite") {
      return t("sftp.editor.overwriteChanged");
    }
    return dirtyEditorDialog.value.kind === "exit"
      ? t("sftp.editor.unsavedExitDescription", { count: dirtyEditorDialog.value.count })
      : t("sftp.editor.unsavedFileDescription", { name: dirtyEditorDialog.value.tab?.name || "" });
  });

  const editorConfirmText = computed(() =>
    dirtyEditorDialog.value.kind === "overwrite"
      ? t("sftp.editor.overwriteAction")
      : t("actions.save"),
  );

  const editorConfirmSecondaryText = computed(() =>
    dirtyEditorDialog.value.kind === "overwrite" ? "" : t("sftp.editor.discardChanges"),
  );

  function resetDirtyEditorDialog() {
    dirtyEditorDialog.value = createDirtyEditorDialogState();
  }

  function resetConflictDialog() {
    conflictDialog.value = createConflictDialogState();
  }

  function requestDirtyEditorAction(payload) {
    return new Promise((resolve) => {
      dirtyEditorDialog.value = {
        count: payload.count || payload.tabs?.length || 0,
        kind: payload.kind || "close",
        open: true,
        resolver: resolve,
        tab: payload.tab || null,
      };
    });
  }

  function requestOverwriteEditorAction(payload) {
    return new Promise((resolve) => {
      dirtyEditorDialog.value = {
        count: 0,
        kind: "overwrite",
        open: true,
        resolver: resolve,
        tab: payload.tab || null,
      };
    });
  }

  function requestUploadConflictAction(payload) {
    return new Promise((resolve) => {
      conflictDialog.value = {
        description: t("sftp.uploadOverwriteMessage", {
          name: payload.name || payload.entry?.name || "",
        }),
        kind: "upload",
        name: payload.name || payload.entry?.name || "",
        open: true,
        resolver: resolve,
        title: t("sftp.uploadOverwriteTitle", { name: payload.name || payload.entry?.name || "" }),
      };
    });
  }

  function requestRenameConflictAction(payload) {
    return new Promise((resolve) => {
      conflictDialog.value = {
        description: t("sftp.renameConflictMessage", {
          name: payload.name || payload.entry?.name || "",
        }),
        kind: "rename",
        name: payload.name || payload.entry?.name || "",
        open: true,
        resolver: resolve,
        title: t("sftp.renameConflictTitle", { name: payload.name || payload.entry?.name || "" }),
      };
    });
  }

  function resolveDirtyEditorAction(action) {
    const resolver = dirtyEditorDialog.value.resolver;
    resetDirtyEditorDialog();
    resolver?.(action);
  }

  function resolveConflictAction(action) {
    const resolver = conflictDialog.value.resolver;
    resetConflictDialog();
    resolver?.(action);
  }

  function onDirtyEditorDialogOpenChange(value) {
    if (!value) {
      resolveDirtyEditorAction(dirtyEditorDialog.value.kind === "overwrite" ? false : "cancel");
    }
  }

  function onConflictDialogOpenChange(value) {
    if (!value) resolveConflictAction(conflictDialog.value.kind === "rename" ? "cancel" : "skip");
  }

  // The pane can be destroyed (key rebuild) while a dialog promise is still
  // pending; resolve with each dialog's cancel semantics so callers never hang.
  onBeforeUnmount(() => {
    if (dirtyEditorDialog.value.open) {
      resolveDirtyEditorAction(dirtyEditorDialog.value.kind === "overwrite" ? false : "cancel");
    }
    if (conflictDialog.value.open) {
      resolveConflictAction(conflictDialog.value.kind === "rename" ? "cancel" : "skip");
    }
  });

  return {
    conflictDialog,
    dirtyEditorDialog,
    editorConfirmDescription,
    editorConfirmSecondaryText,
    editorConfirmText,
    editorConfirmTitle,
    onConflictDialogOpenChange,
    onDirtyEditorDialogOpenChange,
    requestDirtyEditorAction,
    requestRenameConflictAction,
    requestOverwriteEditorAction,
    requestUploadConflictAction,
    resolveConflictAction,
    resolveDirtyEditorAction,
  };
}
