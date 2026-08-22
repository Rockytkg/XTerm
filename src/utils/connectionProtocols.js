export const CONNECTION_PROTOCOLS = Object.freeze(["ssh", "telnet", "serial"]);
export const CONNECTION_PROTOCOL = Object.freeze({
  SSH: "ssh",
  TELNET: "telnet",
  SERIAL: "serial",
});
const PASSWORD_ONLY_CREDENTIAL_PROTOCOLS = Object.freeze(["telnet", "serial"]);

function cleanConnectionProtocol(protocol) {
  return String(protocol || "")
    .trim()
    .toLowerCase();
}

export function normalizeConnectionProtocol(protocol) {
  const normalizedProtocol = cleanConnectionProtocol(protocol);
  return CONNECTION_PROTOCOLS.includes(normalizedProtocol)
    ? normalizedProtocol
    : CONNECTION_PROTOCOL.SSH;
}

export function supportsSavedCredential(protocol) {
  return CONNECTION_PROTOCOLS.includes(cleanConnectionProtocol(protocol));
}

export function requiresPasswordCredential(protocol) {
  return PASSWORD_ONLY_CREDENTIAL_PROTOCOLS.includes(normalizeConnectionProtocol(protocol));
}

export function requiresHostKeyVerification(protocol) {
  return normalizeConnectionProtocol(protocol) === CONNECTION_PROTOCOL.SSH;
}

export function isSerialProtocol(protocol) {
  return normalizeConnectionProtocol(protocol) === CONNECTION_PROTOCOL.SERIAL;
}

export function isTelnetProtocol(protocol) {
  return normalizeConnectionProtocol(protocol) === CONNECTION_PROTOCOL.TELNET;
}

export function isSshProtocol(protocol) {
  return normalizeConnectionProtocol(protocol) === CONNECTION_PROTOCOL.SSH;
}

export function protocolDisplayClass(protocol) {
  const normalized = normalizeConnectionProtocol(protocol);
  return {
    "session-card-status-serial": normalized === CONNECTION_PROTOCOL.SERIAL,
    "session-card-status-telnet": normalized === CONNECTION_PROTOCOL.TELNET,
    "session-card-status-ssh": normalized === CONNECTION_PROTOCOL.SSH,
  };
}

function protocolDisplayName(protocol) {
  return normalizeConnectionProtocol(protocol).toUpperCase();
}

export function connectionEndpointLabel(connection) {
  const protocol = normalizeConnectionProtocol(connection?.protocol);
  if (protocol === CONNECTION_PROTOCOL.SERIAL) {
    return `${connection?.serialPort || connection?.port || "-"}:${connection?.baudRate || "auto"}`;
  }

  const host = connection?.host || connection?.port || "-";
  const user = connection?.user ? `${connection.user}@` : "";
  const port = connection?.port ? `:${connection.port}` : "";
  return `${user}${host}${port}`;
}

export function terminalTargetLine(connection) {
  const protocol = protocolDisplayName(connection?.protocol);
  if (isSerialProtocol(connection?.protocol)) {
    const port = connection?.serialPort || connection?.port || connection?.host || "auto";
    const baud = connection?.baudRate || "auto";
    return `${protocol} ${port} @ ${baud}`;
  }

  const host = connection?.host || connection?.name || "";
  const port = connection?.port ? `:${connection.port}` : "";
  return `${protocol} ${host}${port}`.trim();
}
