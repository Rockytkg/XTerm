import { loadCredentialUsages, loadCredentials } from "./credentials";
import { loadWorkspaceBootstrap } from "./workspace";

let credentialContextRequest = null;
let credentialRelationsRequest = null;

function normalizeList(value) {
  return Array.isArray(value) ? value : [];
}

export function loadCredentialContext() {
  if (credentialContextRequest) return credentialContextRequest;
  credentialContextRequest = Promise.all([
    loadCredentials(),
    loadCredentialUsages(),
    loadWorkspaceBootstrap(),
  ])
    .then(([credentials, usages, workspace]) => ({
      credentials: normalizeList(credentials),
      usages: normalizeList(usages),
      connections: normalizeList(workspace?.connections),
    }))
    .finally(() => {
      credentialContextRequest = null;
    });
  return credentialContextRequest;
}

export function loadCredentialRelations() {
  if (credentialRelationsRequest) return credentialRelationsRequest;
  credentialRelationsRequest = Promise.all([loadCredentialUsages(), loadWorkspaceBootstrap()])
    .then(([usages, workspace]) => ({
      usages: normalizeList(usages),
      connections: normalizeList(workspace?.connections),
    }))
    .finally(() => {
      credentialRelationsRequest = null;
    });
  return credentialRelationsRequest;
}
