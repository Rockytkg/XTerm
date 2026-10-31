export const SCRIPT_ERROR_CODES = Object.freeze({
  STOPPED: "SCRIPT_STOPPED",
  WAIT_TIMEOUT: "SCRIPT_WAIT_TIMEOUT",
  COMMAND: "SCRIPT_COMMAND_ERROR",
});

export class ScriptStoppedError extends Error {
  constructor() {
    super("script stopped");
    this.name = "ScriptStoppedError";
    this.code = SCRIPT_ERROR_CODES.STOPPED;
  }
}

export class ScriptWaitTimeoutError extends Error {
  constructor(message, { pattern = "", timeout = 0 } = {}) {
    super(message);
    this.name = "ScriptWaitTimeoutError";
    this.code = SCRIPT_ERROR_CODES.WAIT_TIMEOUT;
    this.pattern = pattern;
    this.timeout = timeout;
  }
}

export class ScriptCommandError extends Error {
  constructor(message, cmd = "") {
    super(message);
    this.name = "ScriptCommandError";
    this.code = SCRIPT_ERROR_CODES.COMMAND;
    this.cmd = cmd;
  }
}

export function serializeScriptError(error) {
  return {
    name: error?.name || "Error",
    message: String(error?.message || error || "script failed"),
    ...(error?.code ? { code: error.code } : {}),
  };
}

export function deserializeScriptError(payload) {
  const error = new Error(payload?.message || "script failed");
  error.name = payload?.name || "Error";
  if (payload?.code) error.code = payload.code;
  return error;
}
