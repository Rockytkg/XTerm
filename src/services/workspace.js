import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const workspaceLogger = createLogger("frontend.workspace.service");

const runWorkspaceRequest = createServiceRunner({
  logger: workspaceLogger,
  module: "workspace",
});

export function loadWorkspaceBootstrap() {
  return runWorkspaceRequest("workspace_bootstrap", undefined, {
    action: "workspace.bootstrap",
    level: "info",
  });
}

export function getConnection(id) {
  return runWorkspaceRequest(
    "connection_get",
    { id },
    {
      action: "connection.get",
      level: "debug",
      successLevel: "debug",
      context: { connectionId: id },
      summarizePayload: () => ({ connectionId: id }),
    },
  );
}

export function createConnection(profile) {
  return runWorkspaceRequest(
    "connection_create",
    { profile },
    {
      action: "connection.create",
      context: {
        protocol: profile?.protocol,
        connectionName: profile?.name,
      },
      summarizePayload: () => ({
        id: profile?.id,
        name: profile?.name,
        protocol: profile?.protocol,
        host: profile?.host,
        port: profile?.port,
        user: profile?.user,
      }),
    },
  );
}

export function updateConnectionProfile(id, profile) {
  return runWorkspaceRequest(
    "connection_update",
    { id, profile },
    {
      action: "connection.update",
      context: {
        connectionId: id,
        protocol: profile?.protocol,
        connectionName: profile?.name,
      },
      summarizePayload: () => ({
        id,
        name: profile?.name,
        protocol: profile?.protocol,
        host: profile?.host,
        port: profile?.port,
        user: profile?.user,
      }),
    },
  );
}

export function reorderConnectionProfiles(order) {
  return runWorkspaceRequest(
    "connection_reorder",
    { order },
    {
      action: "connection.reorder",
      context: {
        count: order?.length ?? 0,
      },
      summarizePayload: () => ({
        count: order?.length ?? 0,
        order: Array.isArray(order) ? order.slice(0, 10) : [],
      }),
    },
  );
}

export function deleteConnection(id) {
  return runWorkspaceRequest(
    "connection_delete",
    { id },
    {
      action: "connection.delete",
      level: "warn",
      successLevel: "info",
      context: { connectionId: id },
      summarizePayload: () => ({ connectionId: id }),
    },
  );
}

export function setConnectionSavedCredential(connectionId, credentialId) {
  return runWorkspaceRequest(
    "connection_set_saved_credential",
    { link: { connectionId, credentialId } },
    {
      action: "connection.set_saved_credential",
      context: { connectionId, credentialId },
      summarizePayload: () => ({ connectionId, credentialId }),
    },
  );
}

export function clearConnectionSavedCredential(connectionId) {
  return runWorkspaceRequest(
    "connection_clear_saved_credential",
    { connectionId },
    {
      action: "connection.clear_saved_credential",
      level: "warn",
      successLevel: "info",
      context: { connectionId },
      summarizePayload: () => ({ connectionId }),
    },
  );
}
