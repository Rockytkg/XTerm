import { invoke } from "@tauri-apps/api/core";
import { createLogger, summarizeValue } from "../../utils/logger.js";
import { createRuntimeId } from "../../utils/runtimeIds.js";

const ipcLogger = createLogger("frontend.ipc");

function inferScope(command) {
  if (command.startsWith("terminal_")) return "frontend.terminal.ipc";
  if (command.startsWith("sftp_")) return "frontend.sftp.ipc";
  if (command.startsWith("connection_") || command === "workspace_bootstrap") {
    return "frontend.workspace.ipc";
  }
  if (command.startsWith("credentials_")) return "frontend.credentials.ipc";
  if (command.startsWith("preferences_") || command === "setting_set") {
    return "frontend.preferences.ipc";
  }
  if (command.includes("proxy")) return "frontend.proxy.ipc";
  if (command.startsWith("path_settings_")) return "frontend.paths.ipc";
  if (command.startsWith("session_recording_")) return "frontend.recording.ipc";
  if (command.startsWith("log_")) return "frontend.logging.ipc";
  if (command.startsWith("terminal_highlight_")) return "frontend.highlight.ipc";
  return "frontend.ipc";
}

function inferStartLevel(command) {
  if (command.includes("list") || command.includes("get") || command.includes("bootstrap")) {
    return "debug";
  }
  return "info";
}

function resolveLogger(scope) {
  return typeof scope === "string" ? createLogger(scope) : scope || ipcLogger;
}

function resolveSummary(value, summary) {
  return typeof summary === "function" ? summary(value) : summarizeValue(value);
}

export function invokeIpc(command, payload) {
  return invokeDetailedIpc(command, payload);
}

export async function invokeDetailedIpc(command, payload, options = {}) {
  const requestId = options.requestId || createRuntimeId();
  const logger = resolveLogger(options.scope || inferScope(command)).withContext({
    requestId,
    command,
    ...(options.context || {}),
  });
  const startLevel = options.level || inferStartLevel(command);
  const start = performance.now();

  logger[startLevel]("request.start", {
    action: options.action || command,
    payload: resolveSummary(payload, options.summarizePayload),
  });

  try {
    const result = payload === undefined ? await invoke(command) : await invoke(command, payload);
    logger[options.successLevel || startLevel]("request.success", {
      action: options.action || command,
      durationMs: Math.round(performance.now() - start),
      result: resolveSummary(result, options.summarizeResult),
    });
    return result;
  } catch (error) {
    logger[options.failureLevel || "error"]("request.failed", {
      action: options.action || command,
      durationMs: Math.round(performance.now() - start),
      error,
      payload: resolveSummary(payload, options.summarizePayload),
    });
    throw error;
  }
}

export function invokeLoggedIpc(command, payload) {
  return invokeDetailedIpc(command, payload, {
    level: "info",
  });
}

export function invokeDebugIpc(command, payload) {
  return invokeDetailedIpc(command, payload, {
    level: "debug",
    successLevel: "debug",
  });
}
