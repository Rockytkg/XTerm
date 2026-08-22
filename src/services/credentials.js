import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const credentialsLogger = createLogger("frontend.credentials.service");

const runCredentialRequest = createServiceRunner({
  logger: credentialsLogger,
  module: "credentials",
});

export async function loadCredentials() {
  const value = await runCredentialRequest("credentials_list", undefined, {
    action: "credentials.list",
    level: "debug",
    successLevel: "debug",
  });
  return Array.isArray(value) ? value : [];
}

export async function loadCredentialUsages() {
  const value = await runCredentialRequest("credentials_usages", undefined, {
    action: "credentials.usages",
    level: "debug",
    successLevel: "debug",
  });
  return Array.isArray(value) ? value : [];
}

export function createCredential(credential) {
  return runCredentialRequest(
    "credentials_create",
    { credential },
    {
      action: "credentials.create",
      context: {
        credentialId: credential?.id,
        credentialName: credential?.name,
        credentialType: credential?.type,
      },
      summarizePayload: () => ({
        id: credential?.id,
        name: credential?.name,
        type: credential?.type,
      }),
    },
  );
}

export function choosePrivateKey(title) {
  return runCredentialRequest(
    "credentials_choose_private_key",
    { title },
    {
      action: "credentials.choose_private_key",
      summarizePayload: () => ({ title }),
    },
  );
}

export function updateCredential(credential) {
  return runCredentialRequest(
    "credentials_update",
    { credential },
    {
      action: "credentials.update",
      context: {
        credentialId: credential?.id,
        credentialName: credential?.name,
        credentialType: credential?.type,
      },
      summarizePayload: () => ({
        id: credential?.id,
        name: credential?.name,
        type: credential?.type,
      }),
    },
  );
}

export function deleteCredential(credentialId) {
  return runCredentialRequest(
    "credentials_delete",
    { credentialId },
    {
      action: "credentials.delete",
      level: "warn",
      successLevel: "info",
      context: { credentialId },
      summarizePayload: () => ({ credentialId }),
    },
  );
}

export function clearCredentialReferences(credentialId) {
  return runCredentialRequest(
    "credentials_clear_references",
    { credentialId },
    {
      action: "credentials.clear_references",
      level: "warn",
      successLevel: "info",
      context: { credentialId },
      summarizePayload: () => ({ credentialId }),
    },
  );
}

export function deleteUnusedCredentials() {
  return runCredentialRequest("credentials_delete_unused", undefined, {
    action: "credentials.delete_unused",
    level: "warn",
    successLevel: "info",
  });
}

export function reorderCredentials(order) {
  return runCredentialRequest(
    "credentials_reorder",
    { order },
    {
      action: "credentials.reorder",
      context: { count: Array.isArray(order) ? order.length : 0 },
      summarizePayload: () => ({ count: Array.isArray(order) ? order.length : 0 }),
    },
  );
}
