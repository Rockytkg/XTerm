import {
  createCredential,
  loadCredentialUsages,
  loadCredentials,
  updateCredential,
} from "../services/credentials";
import { authenticateBackendConnection } from "../services/terminalSessions";
import { loadWorkspaceBootstrap, updateConnectionProfile } from "../services/workspace";
import { createLogger } from "../utils/logger";
import {
  buildCredentialTypeChangeImpact,
  CREDENTIAL_TYPE_CHANGE_ACTION,
} from "../utils/credentialTypeChange";
import { CONNECTION_EVENT } from "./connectionStateMachine";
import { isSshCredentialPromptError } from "./sshCredentialPromptController";
import { resolveTerminalGeometry } from "./terminalGeometry";
import { formatConnectionError } from "./workspaceUtils";
import { isSerialProtocol, isSshProtocol } from "../utils/connectionProtocols";

const logger = createLogger("frontend.workspace.ssh_flow");

function credentialNameFor(connection, authMethod) {
  const base = connection?.name || connection?.host || "SSH";
  return `${base} ${authMethod === "key" ? "key" : "password"}`;
}

function sshCredentialOverride(input) {
  if (input.authMethod === "key") {
    return {
      authMethod: "key",
      username: input.username,
      privateKey: input.privateKey,
      passphrase: input.passphrase || undefined,
    };
  }
  return {
    authMethod: "password",
    username: input.username,
    password: input.password,
  };
}

function credentialPayload(input, name) {
  return {
    credType: input.authMethod,
    name,
    password: input.authMethod === "password" ? input.password : undefined,
    privateKey: input.authMethod === "key" ? input.privateKey : undefined,
    passphrase: input.authMethod === "key" ? input.passphrase : undefined,
  };
}

async function credentialNameById(credentialId, fallback) {
  if (!credentialId) return fallback;
  try {
    const credentials = await loadCredentials();
    return credentials.find((credential) => credential.id === credentialId)?.name || fallback;
  } catch (error) {
    logger.warn("credential_prompt.metadata_lookup_failed", {
      credentialId,
      error: String(error),
    });
    return fallback;
  }
}

function resolvePersistence(input, prompt) {
  if (prompt.saveAllowed === false) return "temporary";
  if (input.credentialPersistence === CREDENTIAL_TYPE_CHANGE_ACTION.CREATE_NEW) {
    return "createNew";
  }
  if (input.credentialPersistence === "createNew") return "createNew";
  if (input.credentialPersistence === "updateExisting" && prompt.currentCredentialId) {
    return "updateExisting";
  }
  return "temporary";
}

async function loadCredentialChangeContext(credentialId) {
  const [credentials, usages, workspace] = await Promise.all([
    loadCredentials(),
    loadCredentialUsages(),
    loadWorkspaceBootstrap(),
  ]);
  const id = String(credentialId || "");
  return {
    credential: (Array.isArray(credentials) ? credentials : []).find((item) => item.id === id),
    connections: Array.isArray(workspace?.connections) ? workspace.connections : [],
    usages: (Array.isArray(usages) ? usages : []).filter((usage) => usage.credentialId === id),
  };
}

function connectionPatchForCredential(connection, input, credentialId) {
  const protocol = connection.protocol;
  const options = {
    terminalType: connection.terminalType,
    encoding: connection.encoding,
    backspaceSends: connection.backspaceSends,
    terminalHighlightEnabled: connection.terminalHighlightEnabled,
    terminalMorePromptCleanup: connection.terminalMorePromptCleanup,
    realtimeEncodingDetection: connection.realtimeEncodingDetection,
  };
  const patch = {
    id: connection.id,
    protocol,
    port: connection.port,
    name: connection.name,
    host: connection.host,
    user: input.username,
    options,
    details: { protocol, authMethod: input.authMethod, savedCredentialId: credentialId },
  };

  if (isSerialProtocol(protocol)) {
    patch.details = {
      protocol,
      authMethod: input.authMethod,
      savedCredentialId: credentialId,
      baudRate: connection.baudRate,
      serialQuickAutoBaud: connection.serialQuickAutoBaud,
      dataBits: connection.dataBits,
      flowControl: connection.flowControl,
      parity: connection.parity,
      stopBits: connection.stopBits,
    };
  } else if (isSshProtocol(protocol)) {
    patch.details = {
      protocol,
      authMethod: input.authMethod,
      savedCredentialId: credentialId,
      jumpHosts: connection.jumpHosts,
    };
  }

  return patch;
}

export function createSshConnectionFlow({
  activeTerminalSize,
  connectionRuntime,
  dispatchConnectionEvent,
  finishConnectionAttempt,
  getConnection,
  getSessionInstance,
  hostKeyPromptController,
  isExpectedCloseError,
  preferences,
  refreshConnectionList,
  reconnect,
  onAuthenticatedResponse,
  requestClose,
  sshCredentialPromptController,
}) {
  const pendingCredentials = new Map();

  function frontendSessionIdFor(connectionId, context = {}) {
    return context.sessionId || connectionId;
  }

  function prepareOpenOptions(connectionId, options = {}) {
    const frontendSessionId = frontendSessionIdFor(connectionId, options);
    if (options.sshCredential) {
      pendingCredentials.set(frontendSessionId, options.sshCredential);
      return { ...options, sshCredential: options.sshCredential };
    }
    pendingCredentials.delete(frontendSessionId);
    return { ...options };
  }

  function clearConnection(connectionId, sessionId = "") {
    hostKeyPromptController.cancelPromptForConnection(connectionId, sessionId);
    sshCredentialPromptController?.cancelPromptForConnection(connectionId, sessionId);
    if (sessionId) pendingCredentials.delete(sessionId);
  }

  function handleOpenResponse(connectionId, response, context = {}) {
    const frontendSessionId = frontendSessionIdFor(connectionId, context);
    if (response?.awaiting !== "hostKeyChallenge") {
      pendingCredentials.delete(frontendSessionId);
    }
  }

  function handleOpenError(connectionId, error, context = {}) {
    const formatted = formatConnectionError(error);
    const frontendSessionId = frontendSessionIdFor(connectionId, context);
    if (
      connectionRuntime.isCurrent(frontendSessionId, context.attemptToken) &&
      isSshCredentialPromptError(formatted)
    ) {
      pendingCredentials.delete(frontendSessionId);
      return (
        sshCredentialPromptController?.setPromptForError(connectionId, formatted, {
          attemptToken: context.attemptToken,
          sessionId: context.sessionId || "",
        }) ?? false
      );
    }
    return false;
  }

  async function saveCredentialSelectionAndReconnect(connectionId, profile, sessionId = "") {
    const details = profile?.details ?? {};
    const credentialId = details.savedCredentialId;
    const current = getConnection(connectionId);
    const profileChanged =
      !current ||
      current.user !== profile.user ||
      current.authMethod !== details.authMethod ||
      current.savedCredentialId !== credentialId;

    if (profileChanged) {
      await updateConnectionProfile(connectionId, profile);
    }
    await refreshConnectionList();
    reconnect(connectionId, { forceReconnect: true, preserveActiveTab: true, sessionId });
  }

  async function persistCredentialPrompt(prompt, input, persistence) {
    const connectionId = prompt.connectionId;
    if (prompt.sessionId && !getSessionInstance?.(prompt.sessionId)) return false;
    const current = getConnection(connectionId);
    if (!current) return false;

    if (persistence === "createNew") {
      const credential = await createCredential(
        credentialPayload(input, credentialNameFor(current, input.authMethod)),
      );
      await saveCredentialSelectionAndReconnect(
        connectionId,
        connectionPatchForCredential(current, input, credential.id),
        prompt.sessionId,
      );
      return true;
    }

    if (persistence === "updateExisting" && prompt.currentCredentialId) {
      const fallbackName = credentialNameFor(current, input.authMethod);
      const name = await credentialNameById(prompt.currentCredentialId, fallbackName);
      const credential = await updateCredential({
        id: prompt.currentCredentialId,
        ...credentialPayload(input, name),
      });
      await saveCredentialSelectionAndReconnect(
        connectionId,
        connectionPatchForCredential(current, input, credential.id),
        prompt.sessionId,
      );
      return true;
    }

    reconnect(connectionId, {
      forceReconnect: true,
      preserveActiveTab: true,
      sessionId: prompt.sessionId,
      sshCredential: sshCredentialOverride(input),
    });
    return true;
  }

  async function maybeRequestTypeChangeConfirmation(prompt, input, persistence) {
    if (persistence !== "updateExisting" || input.typeChangeAction || !prompt.currentCredentialId) {
      return false;
    }
    const context = await loadCredentialChangeContext(prompt.currentCredentialId);
    const impact = buildCredentialTypeChangeImpact({
      credential: context.credential,
      nextType: input.authMethod,
      usages: context.usages,
      connections: context.connections,
    });
    if (!impact.needsConfirmation) return false;
    sshCredentialPromptController?.updatePrompt({
      ...prompt,
      typeChangeConfirm: impact,
      pendingInput: input,
    });
    return true;
  }

  async function answerCredentialPrompt(input) {
    const prompt = sshCredentialPromptController?.answerPrompt();
    if (!prompt) return false;
    const connectionId = prompt.connectionId;
    const current = getConnection(connectionId);
    if (!current) return false;
    const persistence = resolvePersistence(input, prompt);

    logger.info("credential_prompt.answered", {
      connectionId,
      persistence,
      authMethod: input.authMethod,
    });

    try {
      if (await maybeRequestTypeChangeConfirmation(prompt, input, persistence)) return true;
      await persistCredentialPrompt(prompt, input, persistence);
      return true;
    } catch (error) {
      logger.error("credential_prompt.submit_failed", error);
      dispatchConnectionEvent(eventTargetForPrompt(prompt), {
        type: CONNECTION_EVENT.OPEN_FAILED,
        payload: { error: formatConnectionError(error) },
      });
      return false;
    }
  }

  function cancelCredentialPrompt() {
    const prompt = sshCredentialPromptController?.answerPrompt();
    if (!prompt) return false;
    logger.warn("credential_prompt.cancelled", {
      connectionId: prompt.connectionId,
      reason: prompt.reason,
    });
    if (prompt.sessionId) pendingCredentials.delete(prompt.sessionId);
    dispatchConnectionEvent(eventTargetForPrompt(prompt), {
      type: CONNECTION_EVENT.OPEN_FAILED,
      payload: { error: prompt.error },
    });
    return true;
  }

  function answerHostKeyPrompt(mode) {
    const answered = hostKeyPromptController.answerPrompt(mode);
    if (!answered?.prompt) return false;
    const { prompt, mode: resolvedMode } = answered;
    if (resolvedMode === "cancel") {
      logger.warn("host_key_prompt.cancelled", {
        connectionId: prompt.connectionId,
      });
      requestClose(prompt.connectionId, {
        openRequestId: prompt.openRequestId || prompt.sessionId || prompt.connectionId,
      }).catch((error) => {
        if (!isExpectedCloseError(error)) {
          logger.error("Failed to cancel pending SSH host-key connection", error);
        }
      });
      connectionRuntime.cancel(prompt.sessionId || prompt.connectionId, prompt.attemptToken);
      finishConnectionAttempt?.(prompt.sessionId || prompt.connectionId, prompt.attemptToken);
      if (prompt.sessionId) pendingCredentials.delete(prompt.sessionId);
      dispatchConnectionEvent(prompt.sessionId || prompt.connectionId, {
        type: CONNECTION_EVENT.HOST_KEY_CANCELLED,
      });
      return true;
    }

    logger.info("host_key_prompt.answered", {
      connectionId: prompt.connectionId,
      mode: resolvedMode,
    });
    const frontendSessionId = prompt.sessionId || prompt.connectionId;
    const backendOpenRequestId = prompt.openRequestId || frontendSessionId;
    if (!connectionRuntime.isCurrent(frontendSessionId, prompt.attemptToken)) {
      pendingCredentials.delete(frontendSessionId);
      return false;
    }
    authenticateBackendConnection(prompt.connectionId, {
      openRequestId: backendOpenRequestId,
      trustHostKey: resolvedMode === "save",
      acceptHostKeyOnce: resolvedMode === "once",
      terminalScrollback: preferences.value.terminalScrollback,
      sshCredential: pendingCredentials.get(frontendSessionId),
      ...resolveTerminalGeometry(activeTerminalSize),
    })
      .then((response) => {
        onAuthenticatedResponse?.(prompt.connectionId, response, {
          attemptToken: prompt.attemptToken,
          sessionId: prompt.sessionId || "",
        });
        pendingCredentials.delete(prompt.sessionId || prompt.connectionId);
      })
      .catch((error) => {
        logger.error("Failed to answer host key prompt", error);
        pendingCredentials.delete(prompt.sessionId || prompt.connectionId);
        const context = {
          attemptToken: prompt.attemptToken,
          sessionId: prompt.sessionId || "",
        };
        if (!handleOpenError(prompt.connectionId, error, context)) {
          dispatchConnectionEvent(frontendSessionId, {
            type: CONNECTION_EVENT.HOST_KEY_AUTH_FAILED,
            payload: { error: formatConnectionError(error) },
          });
        }
      })
      .finally(() => {
        finishConnectionAttempt?.(frontendSessionId, prompt.attemptToken);
        connectionRuntime.finish(frontendSessionId, prompt.attemptToken);
      });
    return true;
  }

  return {
    answerCredentialPrompt,
    answerHostKeyPrompt,
    cancelCredentialPrompt,
    clearConnection,
    handleOpenError,
    handleOpenResponse,
    prepareOpenOptions,
  };
}
function eventTargetForPrompt(prompt) {
  return prompt?.sessionId || prompt?.connectionId || "";
}
