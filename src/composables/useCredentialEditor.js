import { computed, ref } from "vue";
import { choosePrivateKey, createCredential, updateCredential } from "../services/credentials";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.credentials.editor");

function defaultPasswordForm() {
  return { name: "", password: "" };
}

function defaultKeyForm() {
  return {
    name: "",
    privateKey: "",
    passphrase: "",
    comment: "",
  };
}

function credentialPayload(type, passwordForm, keyForm) {
  if (type === "password") {
    return {
      credType: "password",
      name: passwordForm.name.trim(),
      password: passwordForm.password,
    };
  }

  return {
    credType: "key",
    name: keyForm.name.trim(),
    privateKey: keyForm.privateKey,
    passphrase: keyForm.passphrase,
    comment: keyForm.comment.trim(),
  };
}

export function useCredentialEditor({ t, showToast }) {
  const addingType = ref(null);
  const credentialDialogOpen = ref(false);
  const editingCredential = ref(null);
  const editingId = ref(null);
  const formError = ref("");
  const passwordForm = ref(defaultPasswordForm());
  const keyForm = ref(defaultKeyForm());

  const canSave = computed(() => {
    if (addingType.value === "password") {
      const requiresPassword = editingId.value && editingCredential.value?.credType !== "password";
      return passwordForm.value.name.trim() && (!requiresPassword || passwordForm.value.password);
    }
    if (addingType.value === "key") {
      return (
        keyForm.value.name.trim() &&
        ((editingId.value && editingCredential.value?.credType === "key") ||
          keyForm.value.privateKey.trim())
      );
    }
    return false;
  });

  const credentialDialogTitle = computed(() =>
    editingId.value ? t("actions.edit") : t("credentials.add"),
  );

  function resetForms() {
    passwordForm.value = defaultPasswordForm();
    keyForm.value = defaultKeyForm();
  }

  function resetEditorState() {
    addingType.value = null;
    editingCredential.value = null;
    editingId.value = null;
    formError.value = "";
    resetForms();
  }

  function startAdd(type) {
    resetEditorState();
    addingType.value = type;
    credentialDialogOpen.value = true;
  }

  function startEdit(credential) {
    resetEditorState();
    addingType.value = credential.credType;
    editingCredential.value = credential;
    editingId.value = credential.id;
    credentialDialogOpen.value = true;
    passwordForm.value = {
      name: credential.name || "",
      password: "",
    };
    keyForm.value = {
      name: credential.name || "",
      privateKey: "",
      passphrase: "",
      comment: credential.comment || "",
    };
  }

  function closeEditor() {
    credentialDialogOpen.value = false;
    resetEditorState();
  }

  function onCredentialDialogOpenChange(value) {
    credentialDialogOpen.value = value;
    if (!value) resetEditorState();
  }

  function selectCredentialType(type) {
    if (addingType.value === type) return;
    const previousType = addingType.value;
    addingType.value = type;
    formError.value = "";
    if (previousType === "password" && type === "key") {
      keyForm.value = {
        ...keyForm.value,
        name: keyForm.value.name || passwordForm.value.name,
      };
    } else if (previousType === "key" && type === "password") {
      passwordForm.value = {
        ...passwordForm.value,
        name: passwordForm.value.name || keyForm.value.name,
      };
    }
  }

  async function pickPrivateKeyFile() {
    formError.value = "";
    try {
      const privateKey = await choosePrivateKey(t("credentials.fields.choosePrivateKeyTitle"));
      if (privateKey) {
        keyForm.value.privateKey = privateKey;
      }
    } catch (error) {
      formError.value = String(error);
    }
  }

  function buildCredentialSaveRequest({ mode } = {}) {
    formError.value = "";
    logger.info(
      "credential.save.requested",
      addingType.value,
      editingId.value ? "(edit)" : "(new)",
    );

    const type = addingType.value;
    if (type === "password") {
      const requiresPassword = editingId.value && editingCredential.value?.credType !== "password";
      if (!passwordForm.value.name.trim()) return null;
      if (requiresPassword && !passwordForm.value.password) return null;
    }
    if (type === "key") {
      if (!keyForm.value.name.trim()) return null;
      if (
        !(editingId.value && editingCredential.value?.credType === "key") &&
        !keyForm.value.privateKey.trim()
      ) {
        return null;
      }
    }

    const payload = credentialPayload(type, passwordForm.value, keyForm.value);
    const resolvedMode = mode || (editingId.value ? "updateExisting" : "createNew");
    return {
      credential: editingCredential.value,
      mode: resolvedMode,
      payload,
    };
  }

  async function persistCredentialRequest(request) {
    if (!request) return null;
    const saved = await (
      request.mode === "updateExisting"
        ? updateCredential({ id: request.credential.id, ...request.payload })
        : createCredential(request.payload)
    ).catch((error) => {
      formError.value = String(error);
      showToast({
        type: "error",
        title: t("notifications.credentialSaveFailed"),
        message: String(error),
      });
      return null;
    });

    return saved;
  }

  return {
    addingType,
    buildCredentialSaveRequest,
    canSave,
    closeEditor,
    credentialDialogOpen,
    credentialDialogTitle,
    editingCredential,
    editingId,
    formError,
    keyForm,
    onCredentialDialogOpenChange,
    passwordForm,
    persistCredentialRequest,
    pickPrivateKeyFile,
    resetEditorState,
    selectCredentialType,
    startAdd,
    startEdit,
  };
}
