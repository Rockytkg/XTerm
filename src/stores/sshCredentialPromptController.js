import { ref } from "vue";

const CREDENTIAL_PROMPT_CODES = {
  ssh: new Set([
    "ssh_user_required",
    "ssh_credential_required",
    "ssh_auth_rejected",
    "ssh_password_auth_failed",
    "ssh_key_auth_failed",
    "ssh_key_decode_failed",
  ]),
  vnc: new Set(["vnc_auth_required", "vnc_auth_failed"]),
  rdp: new Set(["rdp_username_required", "rdp_auth_required", "rdp_auth_failed"]),
};

const MISSING_CREDENTIAL_CODES = new Set([
  "ssh_user_required",
  "ssh_credential_required",
  "vnc_auth_required",
  "rdp_username_required",
  "rdp_auth_required",
]);

const NEW_CREDENTIAL_ONLY_CODES = new Set([
  "ssh_credential_required",
  "vnc_auth_required",
  "rdp_auth_required",
]);

const DEFAULT_PORTS = { ssh: "22", vnc: "5900", rdp: "3389" };

export function isCredentialPromptError(error, protocol) {
  const code = String(error?.code || error?.errorCode || "").toLowerCase();
  return CREDENTIAL_PROMPT_CODES[protocol]?.has(code) ?? false;
}

export function createSshCredentialPromptController({ getConnection }) {
  const sshCredentialPrompt = ref(null);

  function buildPrompt(connectionId, error, context = {}) {
    const connection = getConnection?.(connectionId);
    const protocol = connection?.protocol || "";
    if (!connection) return null;
    const code = String(error?.code || error?.errorCode || "").toLowerCase();
    if (!isCredentialPromptError({ code }, protocol)) return null;
    const saveAllowed = !connection.external && connection.source !== "transient";
    const currentCredentialId = connection.savedCredentialId || "";
    return {
      attemptToken: context.attemptToken ?? null,
      connectionId,
      sessionId: context.sessionId || "",
      reason: MISSING_CREDENTIAL_CODES.has(code) ? "missing" : "rejected",
      saveAllowed,
      currentCredentialId,
      canUpdateCredential:
        saveAllowed && !!currentCredentialId && !NEW_CREDENTIAL_ONLY_CODES.has(code),
      usernameRequired: context.usernameRequired ?? protocol !== "vnc",
      onAnswer: typeof context.onAnswer === "function" ? context.onAnswer : null,
      error,
      connection: {
        id: connection.id,
        protocol,
        name: connection.name || connection.host || connection.id,
        host: connection.host || "",
        port: connection.port || DEFAULT_PORTS[protocol] || "",
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
