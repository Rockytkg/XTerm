function normalizeBackendSessionId(value) {
  return typeof value === "string" ? value.trim() : "";
}

function normalizeTerminalChannelId(value) {
  const channelId = Number(value);
  return Number.isFinite(channelId) ? channelId : null;
}

export function createTerminalChannelLease({
  sessionId,
  channelId,
  subscriptionId = null,
  connectionId = "",
  alreadyActive = false,
} = {}) {
  return {
    alreadyActive: alreadyActive === true,
    channelId: normalizeTerminalChannelId(channelId),
    connectionId: typeof connectionId === "string" ? connectionId : "",
    sessionId: normalizeBackendSessionId(sessionId),
    subscriptionId,
  };
}

export function leaseKey(lease) {
  if (!lease?.sessionId || lease.channelId === null || lease.channelId === undefined) return "";
  return `${lease.sessionId}:${lease.channelId}:${lease.subscriptionId ?? ""}`;
}

export function leaseOwnsPayload(lease, payload) {
  if (!lease?.sessionId || !payload || payload.sessionId !== lease.sessionId) return false;
  if (payload.channelId === null || payload.channelId === undefined) return true;
  if (lease.channelId === null || lease.channelId === undefined) return true;
  return Number(payload.channelId) === Number(lease.channelId);
}
