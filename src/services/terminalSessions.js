import { invokeDetailedIpc, invokeLoggedIpc } from "./ipc/core";
import { createLogger, summarizeValue } from "../utils/logger";

const terminalLogger = createLogger("frontend.terminal.service");

function summarizeTerminalResult(value) {
  return summarizeValue(value);
}

function invokeTerminal(command, payload = {}, options = {}) {
  return invokeDetailedIpc(command, payload, {
    scope: terminalLogger,
    level: options.level || "info",
    successLevel: options.successLevel || "debug",
    failureLevel: options.failureLevel || "error",
    summarizePayload: options.summarizePayload || (() => summarizeValue(payload)),
    summarizeResult: options.summarizeResult || summarizeTerminalResult,
  });
}

export function deleteSshHostKey(request) {
  return invokeLoggedIpc("ssh_host_key_delete", { request });
}

export function openBackendConnection(connectionId, options = {}) {
  const request = { connectionId, ...options };
  return invokeTerminal(
    "terminal_connection_open",
    { request },
    {
      summarizePayload: () => ({
        connectionId,
        openRequestId: request.openRequestId,
        trustHostKey: request.trustHostKey,
        acceptHostKeyOnce: request.acceptHostKeyOnce,
        terminalScrollback: request.terminalScrollback,
        terminalType: request.terminalType,
        cols: request.cols,
        rows: request.rows,
        sshCredential: request.sshCredential
          ? { authMethod: request.sshCredential.authMethod }
          : undefined,
      }),
    },
  );
}

export function authenticateBackendConnection(connectionId, options = {}) {
  const request = { connectionId, ...options };
  return invokeTerminal(
    "terminal_connection_authenticate",
    { request },
    {
      summarizePayload: () => ({
        connectionId,
        openRequestId: request.openRequestId,
        trustHostKey: request.trustHostKey,
        acceptHostKeyOnce: request.acceptHostKeyOnce,
        terminalScrollback: request.terminalScrollback,
        cols: request.cols,
        rows: request.rows,
        sshCredential: request.sshCredential
          ? { authMethod: request.sshCredential.authMethod }
          : undefined,
      }),
    },
  );
}

export function closeBackendConnection(connectionId) {
  return invokeTerminal(
    "terminal_connection_close",
    { request: { connectionId } },
    {
      summarizePayload: () => ({ connectionId }),
    },
  );
}

export function cancelBackendConnectionOpen(connectionId, options = {}) {
  return invokeTerminal(
    "terminal_connection_open_cancel",
    { request: { connectionId, openRequestId: options.openRequestId } },
    {
      summarizePayload: () => ({ connectionId, openRequestId: options.openRequestId }),
    },
  );
}

export function closeBackendSession(sessionId) {
  return invokeTerminal(
    "terminal_session_close",
    { request: { sessionId } },
    {
      summarizePayload: () => ({ sessionId }),
    },
  );
}

export function setBackendEncodingDetection(request) {
  return invokeTerminal(
    "terminal_session_set_encoding_detection",
    { request },
    {
      summarizePayload: () => ({
        sessionId: request?.sessionId,
        channelId: request?.channelId,
        enabled: request?.enabled,
        encoding: request?.encoding,
      }),
    },
  );
}

export function redetectBackendSerialBaud(sessionId) {
  return invokeTerminal(
    "terminal_serial_redetect_baud",
    { request: { sessionId } },
    {
      summarizePayload: () => ({ sessionId }),
    },
  );
}

export function startSshRuntimeMetrics(connectionId, sessionId) {
  const request = { connectionId, sessionId };
  return invokeTerminal(
    "terminal_metrics_start",
    { request },
    {
      summarizePayload: () => request,
    },
  );
}

export function stopSshRuntimeMetrics(connectionId, sessionId) {
  const request = { connectionId, sessionId };
  return invokeTerminal(
    "terminal_metrics_stop",
    { request },
    {
      failureLevel: "warn",
      summarizePayload: () => request,
    },
  );
}
