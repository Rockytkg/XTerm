const SERIAL_LINK_FIELDS = [
  "protocol",
  "host",
  "port",
  "user",
  "serialPort",
  "baudRate",
  "serialQuickAutoBaud",
  "dataBits",
  "flowControl",
  "parity",
  "stopBits",
  "encoding",
  "authMethod",
  "savedCredentialId",
];

export function serialConnectionProfileKey(connection) {
  return JSON.stringify(SERIAL_LINK_FIELDS.map((field) => connection?.[field] ?? null));
}

export function findReusableSerialSession(sessions, profileKeyBySession, connection) {
  const currentProfileKey = serialConnectionProfileKey(connection);
  return sessions.find((session) => profileKeyBySession.get(session.id) === currentProfileKey);
}
