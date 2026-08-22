import { ref } from "vue";
import { requiresHostKeyVerification } from "../utils/connectionProtocols";

const SSH_CREDENTIAL_PROMPT_CODES = new Set([
  "ssh_user_required",
  "ssh_credential_required",
  "ssh_auth_rejected",
  "ssh_password_auth_failed",
  "ssh_key_auth_failed",
  "ssh_key_decode_failed",
]);

export function isSshCredentialPromptError(error) {
  const code = String(error?.code || error?.errorCode || "").toLowerCase();
  return SSH_CREDENTIAL_PROMPT_CODES.has(code);
}

export function createSshCredentialPromptController({ getConnection }) {
  const sshCredentialPrompt = ref(null);

  function buildPrompt(connectionId, error, context = {}) {
    const connection = getConnection?.(connectionId);
    if (!connection || !requiresHostKeyVerification(connection.protocol)) return null;
    const code = String(error?.code || error?.errorCode || "").toLowerCase();
    if (!SSH_CREDENTIAL_PROMPT_CODES.has(code)) return null;
    const saveAllowed = !connection.external && connection.source !== "transient";
    const currentCredentialId = connection.savedCredentialId || "";
    return {
      attemptToken: context.attemptToken ?? null,
      connectionId,
      sessionId: context.sessionId || "",
      reason: ["ssh_credential_required", "ssh_user_required"].includes(code)
        ? "missing"
        : "rejected",
      saveAllowed,
      currentCredentialId,
      canUpdateCredential:
        saveAllowed && !!currentCredentialId && code !== "ssh_credential_required",
      error,
      connection: {
        id: connection.id,
        name: connection.name || connection.host || connection.id,
        host: connection.host || "",
        port: connection.port || "22",
        user: connection.user || "",
        authMethod: connection.authMethod || "password",
      },
    };
  }

  function setPromptForError(connectionId, error, context = {}) {
    const prompt = buildPrompt(connectionId, error, context);
    if (!prompt) return false;
    sshCredentialPrompt.value = prompt;
    return true;
  }

  function answerPrompt() {
    const prompt = sshCredentialPrompt.value;
    sshCredentialPrompt.value = null;
    return prompt;
  }

  function updatePrompt(patch) {
    if (!sshCredentialPrompt.value && !patch?.connectionId) return false;
    sshCredentialPrompt.value = { ...(sshCredentialPrompt.value || {}), ...patch };
    return true;
  }

  function cancelPromptForConnection(connectionId, sessionId = "") {
    if (sshCredentialPrompt.value?.connectionId !== connectionId) return false;
    if (sessionId && sshCredentialPrompt.value?.sessionId !== sessionId) return false;
    sshCredentialPrompt.value = null;
    return true;
  }

  return {
    answerPrompt,
    cancelPromptForConnection,
    setPromptForError,
    sshCredentialPrompt,
    updatePrompt,
  };
}
