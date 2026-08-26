import { computed, ref } from "vue";
import { clearCredentialReferences, deleteCredential } from "../services/credentials";
import { useDialogExitTeardown } from "./useDialogExitTeardown";

export function normalizeCredentialUsages(usages, credentialId) {
  const seen = new Set();
  return (Array.isArray(usages) ? usages : [])
    .filter((usage) => usage?.credentialId === credentialId && usage.connectionId)
    .map((usage) => ({
      connectionId: usage.connectionId,
      connectionName: usage.connectionName || usage.connectionId,
      relation: usage.relation || "connection",
    }))
    .filter((usage) => {
      const key = `${usage.connectionId}:${usage.relation}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

export function useCredentialDeleteFlow({ t, showToast, getUsages, onDeleted, onFailed } = {}) {
  const credentialDeleteOpen = ref(false);
  const pendingCredentialDelete = ref(null);
  const credentialDeleteBusy = ref(false);
  const { scheduleExitTeardown, cancelExitTeardown } = useDialogExitTeardown();

  const pendingCredentialDeleteDescription = computed(() => {
    const pending = pendingCredentialDelete.value;
    if (!pending) return "";
    const key = pending.usages.length
      ? "relationshipGraph.confirm.credentialDelete.usedDescription"
      : "relationshipGraph.confirm.credentialDelete.description";
    return t(key, {
      name: pending.name,
      count: pending.usages.length,
    });
  });

  function requestCredentialDelete(credential = {}) {
    if (!credential.id) return;
    cancelExitTeardown();
    pendingCredentialDelete.value = {
      id: credential.id,
      name: credential.name || credential.title || credential.id,
      usages: typeof getUsages === "function" ? getUsages(credential.id) : [],
    };
    credentialDeleteOpen.value = true;
  }

  function closeCredentialDeleteDialog() {
    credentialDeleteOpen.value = false;
    // 退出动画仍在渲染确认内容，pending 数据延迟到动画结束后清空
    scheduleExitTeardown(() => {
      pendingCredentialDelete.value = null;
    });
  }

  async function confirmCredentialDelete() {
    const pending = pendingCredentialDelete.value;
    if (!pending || credentialDeleteBusy.value) return;
    credentialDeleteBusy.value = true;
    try {
      if (pending.usages.length) {
        await clearCredentialReferences(pending.id);
      }
      await deleteCredential(pending.id);
      closeCredentialDeleteDialog();
      await onDeleted?.(pending);
      showToast?.({ type: "success", title: t("notifications.credentialDeleted") });
    } catch (error) {
      await onFailed?.(error, pending);
      showToast?.({
        type: "error",
        title: t("notifications.credentialDeleteFailed"),
        message: String(error),
      });
    } finally {
      credentialDeleteBusy.value = false;
    }
  }

  function cancelCredentialDelete() {
    if (credentialDeleteBusy.value) return;
    closeCredentialDeleteDialog();
  }

  return {
    credentialDeleteOpen,
    pendingCredentialDelete,
    credentialDeleteBusy,
    pendingCredentialDeleteDescription,
    requestCredentialDelete,
    confirmCredentialDelete,
    cancelCredentialDelete,
  };
}
