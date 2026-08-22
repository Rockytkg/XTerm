import { requiresPasswordCredential } from "./connectionProtocols";

export const CREDENTIAL_TYPE_CHANGE_ACTION = Object.freeze({
  UPDATE_EXISTING: "updateExisting",
  CREATE_NEW: "createNew",
  CANCEL: "cancel",
});

export function credentialTypeChanged(credential, nextType) {
  return !!credential?.id && !!nextType && credential.credType !== nextType;
}

export function passwordOnlyCredentialUsages(usages = [], connections = []) {
  const connectionById = new Map(
    (Array.isArray(connections) ? connections : [])
      .filter((connection) => connection?.id)
      .map((connection) => [connection.id, connection]),
  );
  const affected = [];
  const seen = new Set();

  for (const usage of Array.isArray(usages) ? usages : []) {
    const connection = connectionById.get(usage?.connectionId);
    if (!connection || !requiresPasswordCredential(connection.protocol)) continue;
    const key = `${usage.connectionId}:${usage.relation || "connection"}`;
    if (seen.has(key)) continue;
    seen.add(key);
    affected.push({
      ...usage,
      connectionName: usage.connectionName || connection.name || connection.id,
      protocol: connection.protocol,
    });
  }

  return affected;
}

export function buildCredentialTypeChangeImpact({
  credential,
  nextType,
  usages = [],
  connections = [],
}) {
  const changesType = credentialTypeChanged(credential, nextType);
  const affectsPasswordOnlyConnections =
    changesType && credential?.credType === "password" && nextType === "key";
  const affectedUsages = affectsPasswordOnlyConnections
    ? passwordOnlyCredentialUsages(usages, connections)
    : [];

  return {
    affectedUsages,
    changesType,
    needsConfirmation: affectedUsages.length > 0,
  };
}
