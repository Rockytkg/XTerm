export const EMPTY_CONNECTION_CAPABILITIES = Object.freeze({
  shell: false,
  exec: false,
  subsystem: false,
  sftp: false,
  metrics: false,
  resize: false,
  encodingDetection: false,
  serialSignals: false,
  rawOutput: false,
  serialBaudDetection: false,
});

export function normalizeConnectionCapabilities(capabilities) {
  return Object.freeze({
    ...EMPTY_CONNECTION_CAPABILITIES,
    ...(capabilities || {}),
  });
}

export function capabilitiesCan(capabilities, capability) {
  return !!capabilities?.[capability];
}

export function connectionCan(connection, capability) {
  return capabilitiesCan(connection?.capabilities, capability);
}

export function runtimeCan(runtime, capability) {
  return capabilitiesCan(runtime?.capabilities, capability);
}
