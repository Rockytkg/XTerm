import {
  CONNECTION_PROTOCOL,
  CONNECTION_PROTOCOLS,
  isSerialProtocol,
  isTelnetProtocol,
  normalizeConnectionProtocol,
  requiresHostKeyVerification,
} from "../../utils/connectionProtocols";
import {
  BACKSPACE_SENDS,
  DEFAULT_TERMINAL_ENCODING,
  DEFAULT_TERMINAL_TYPE,
} from "../../utils/terminalSessionOptions";

const PROTOCOL_PORT_DEFAULTS = {
  ssh: 22,
  telnet: 23,
  serial: null,
};

function enabledOrDefault(value, fallback = true) {
  if (value === undefined || value === null) return fallback;
  return value !== false;
}

function defaultTerminalHighlightEnabled(profile, protocol) {
  const value = profile?.options?.terminalHighlightEnabled;
  if (value !== undefined && value !== null) {
    return value !== false;
  }
  return requiresHostKeyVerification(protocol) ? false : true;
}

function commonDraftFromProfile(profile, protocol) {
  const options = profile?.options ?? {};
  return {
    name: profile?.name ?? "",
    terminalHighlightEnabled: defaultTerminalHighlightEnabled(profile, protocol),
    terminalType: options.terminalType || DEFAULT_TERMINAL_TYPE,
    encoding: options.encoding || DEFAULT_TERMINAL_ENCODING,
    backspaceSends: options.backspaceSends || BACKSPACE_SENDS.DEL,
    terminalMorePromptCleanup: options.terminalMorePromptCleanup === true,
  };
}

function profileDetails(profile) {
  const details = profile?.details ?? {};
  // The backend serializes the tagged union as { protocol, ...fields }.
  // We return the fields directly so callers can read authMethod, etc.
  const { protocol: _protocol, ...fields } = details;
  return fields;
}

function normalizeJumpHost(hop = {}) {
  const connectionId = hop?.connectionId?.trim?.() || "";
  return {
    source: hop?.source || (connectionId ? "connection" : "manual"),
    connectionId,
    host: hop?.host?.trim?.() || "",
    port: hop?.port ? String(hop.port).trim() : "",
    user: hop?.user?.trim?.() || "",
    authMethod: hop?.authMethod || "password",
    savedCredentialId: hop?.savedCredentialId || "",
  };
}

export function createProtocolDraft(protocol, profile = null) {
  const normalizedProtocol = normalizeConnectionProtocol(protocol);
  const common = commonDraftFromProfile(profile, normalizedProtocol);
  const details = profileDetails(profile);

  if (isSerialProtocol(normalizedProtocol)) {
    return {
      ...common,
      user: profile?.user ?? "",
      password: "",
      authMethod: details.authMethod || "password",
      savedCredentialId: details.savedCredentialId || "",
      serialPort: profile?.port || profile?.host || "auto",
      baudRate: details.baudRate ?? "auto",
      dataBits: details.dataBits ?? 8,
      flowControl: details.flowControl || "none",
      parity: details.parity || "none",
      stopBits: details.stopBits ?? 1,
      serialQuickAutoBaud: enabledOrDefault(details.serialQuickAutoBaud),
    };
  }

  if (isTelnetProtocol(normalizedProtocol)) {
    return {
      ...common,
      host: profile?.host ?? "",
      port: profile?.port ?? PROTOCOL_PORT_DEFAULTS.telnet,
      user: profile?.user ?? "",
      password: "",
      authMethod: details.authMethod || "password",
      savedCredentialId: details.savedCredentialId || "",
    };
  }

  return {
    ...common,
    host: profile?.host ?? "",
    port: profile?.port ?? PROTOCOL_PORT_DEFAULTS.ssh,
    user: profile?.user ?? "",
    password: "",
    privateKey: "",
    keyPassphrase: "",
    authMethod: details.authMethod || "password",
    savedCredentialId: details.savedCredentialId || "",
    jumpHosts:
      Array.isArray(details.jumpHosts) && details.jumpHosts.length
        ? details.jumpHosts.map((hop) => normalizeJumpHost(hop))
        : [],
  };
}

export function resetProtocolDrafts(drafts, profile = null) {
  CONNECTION_PROTOCOLS.forEach((protocol) => {
    Object.assign(drafts[protocol], createProtocolDraft(protocol));
  });

  if (profile) {
    const protocol = normalizeConnectionProtocol(profile.protocol);
    Object.assign(drafts[protocol], createProtocolDraft(protocol, profile));
    return protocol;
  }

  return CONNECTION_PROTOCOL.SSH;
}

export function buildConnectionProfile({
  baseProfile: _baseProfile = {},
  id,
  protocol,
  form,
  savedCredentialId,
}) {
  const encodingValue =
    form.encoding === DEFAULT_TERMINAL_ENCODING ? undefined : form.encoding || undefined;
  const options = {
    terminalType: form.terminalType || undefined,
    encoding: encodingValue,
    backspaceSends:
      form.backspaceSends === BACKSPACE_SENDS.DEL ? undefined : form.backspaceSends || undefined,
    terminalHighlightEnabled: form.terminalHighlightEnabled,
    terminalMorePromptCleanup: form.terminalMorePromptCleanup === true ? true : undefined,
  };
  const commonProfile = {
    id,
    protocol,
    name: form.name.trim(),
    options,
  };

  if (isSerialProtocol(protocol)) {
    return {
      ...commonProfile,
      port: form.serialPort,
      host: form.serialPort === "auto" ? "" : form.serialPort,
      user: form.user?.trim?.() ?? "",
      details: {
        protocol,
        authMethod: "password",
        savedCredentialId: savedCredentialId || undefined,
        baudRate: form.baudRate === "auto" ? "auto" : Number(form.baudRate),
        dataBits: Number(form.dataBits),
        flowControl: form.flowControl,
        parity: form.parity,
        stopBits: Number(form.stopBits),
        serialQuickAutoBaud: form.serialQuickAutoBaud !== false,
      },
    };
  }

  if (isTelnetProtocol(protocol)) {
    return {
      ...commonProfile,
      host: form.host.trim(),
      port: String(Number(form.port)),
      user: form.user?.trim?.() ?? "",
      details: {
        protocol,
        authMethod: "password",
        savedCredentialId: savedCredentialId || undefined,
      },
    };
  }

  return {
    ...commonProfile,
    host: form.host.trim(),
    port: String(Number(form.port)),
    user: form.user.trim(),
    details: {
      protocol,
      authMethod: form.authMethod,
      savedCredentialId: savedCredentialId || undefined,
      jumpHosts: Array.isArray(form.jumpHosts)
        ? form.jumpHosts
            .map((hop) => {
              const source = hop.source || (hop.connectionId ? "connection" : "manual");
              if (source === "connection") {
                return {
                  connectionId: hop.connectionId?.trim?.() || undefined,
                  host: "",
                };
              }
              return {
                host: hop.host?.trim?.() || "",
                port: hop.port ? String(Number(hop.port)) : undefined,
                user: hop.user?.trim?.() || undefined,
                authMethod: hop.authMethod || undefined,
                savedCredentialId: hop.savedCredentialId || undefined,
              };
            })
            .filter((hop) => hop.connectionId || hop.host)
        : undefined,
    },
  };
}
