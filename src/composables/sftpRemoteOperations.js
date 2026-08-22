import { listRemoteSftp, renameRemoteSftp } from "../services/sftp";

export const NAME_CONFLICT_ACTION = {
  CANCEL: "cancel",
  CREATE: "create",
  OVERWRITE: "overwrite",
  RESUME: "resume",
  SKIP: "skip",
};

function mapEntriesByName(entries) {
  return new Map((entries || []).map((entry) => [entry.name, entry]));
}

export async function loadRemoteEntriesByName({ connectionId, sessionId, path }) {
  if (!connectionId || !sessionId) return new Map();
  const result = await listRemoteSftp(connectionId, sessionId, path);
  return mapEntriesByName(result?.entries || []);
}

export async function resolveNameConflict({
  sourcePath = "",
  targetFileByName,
  targetName,
  requestConflictAction,
  defaultAction = NAME_CONFLICT_ACTION.CREATE,
  skipAction = NAME_CONFLICT_ACTION.SKIP,
  sourceEntry = null,
}) {
  const existing = targetFileByName?.get(targetName) || null;
  if (!existing || existing.path === sourcePath) {
    return {
      action: defaultAction,
      existing: null,
      cancelled: false,
      skipped: false,
    };
  }

  const action = await requestConflictAction({
    entry: existing,
    name: targetName,
    sourceEntry,
  });
  return {
    action,
    existing,
    cancelled: action === NAME_CONFLICT_ACTION.CANCEL,
    skipped: action === skipAction,
  };
}

export async function renameRemoteEntry({
  connectionId,
  sessionId,
  fromPath,
  toParentPath,
  toName,
  conflictAction = NAME_CONFLICT_ACTION.CREATE,
}) {
  return renameRemoteSftp(connectionId, sessionId, fromPath, toParentPath, toName, conflictAction);
}
