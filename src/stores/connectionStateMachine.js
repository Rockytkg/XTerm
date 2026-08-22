const CONNECTION_STATUS = Object.freeze({
  AUTHENTICATING: "authenticating",
  CLOSED: "closed",
  CONNECTED: "connected",
  CONNECTING: "connecting",
  DISCONNECTED: "disconnected",
  DISCONNECTING: "disconnecting",
  FAILED: "failed",
  IDLE: "idle",
});

const CONNECTION_PHASE = Object.freeze({
  CONNECTING: "connecting",
  DISCONNECTING: "disconnecting",
  HOST_KEY_CHALLENGE: "hostKeyChallenge",
  SERIAL_BAUD_DETECTION: "serialBaudDetection",
});

export const CONNECTION_EVENT = Object.freeze({
  CLOSE_REQUESTED: "close.requested",
  HOST_KEY_AUTH_FAILED: "host_key.auth.failed",
  HOST_KEY_CANCELLED: "host_key.cancelled",
  HOST_KEY_CHALLENGE: "host_key.challenge",
  OPEN_FAILED: "open.failed",
  OPEN_REQUESTED: "open.requested",
  SERIAL_METADATA_RECEIVED: "serial.metadata.received",
  SERIAL_REDETECT_FAILED: "serial.redetect.failed",
  SERIAL_REDETECT_REQUESTED: "serial.redetect.requested",
  SERIAL_REDETECT_SUCCEEDED: "serial.redetect.succeeded",
  SESSION_CLOSED: "session.closed",
  SESSION_FAILED: "session.failed",
  SESSION_READY: "session.ready",
});

export const IDLE_CONNECTION_STATE = Object.freeze({
  status: CONNECTION_STATUS.IDLE,
  error: null,
  latency: "—",
  statusDetail: "",
});

function diagnosticDetail(error, fallback = "") {
  const detail = typeof error?.detail === "string" ? error.detail.trim() : "";
  return detail || fallback || "";
}

const FINAL_SESSION_STATUSES = new Set([CONNECTION_STATUS.CLOSED, CONNECTION_STATUS.FAILED]);

function isFinalLifecycleState(state) {
  return FINAL_SESSION_STATUSES.has(state?.status);
}

const WARNING_STATUSES = new Set([
  CONNECTION_STATUS.CONNECTING,
  CONNECTION_STATUS.AUTHENTICATING,
  CONNECTION_STATUS.DISCONNECTING,
]);

const OFFLINE_STATUSES = new Set([
  CONNECTION_STATUS.FAILED,
  CONNECTION_STATUS.CLOSED,
  CONNECTION_STATUS.IDLE,
  CONNECTION_STATUS.DISCONNECTED,
]);

export function connectionRuntimeStatus(stateStatus, fallbackStatus = "offline") {
  if (stateStatus === CONNECTION_STATUS.CONNECTED) return "online";
  if (WARNING_STATUSES.has(stateStatus)) {
    return "warning";
  }
  if (OFFLINE_STATUSES.has(stateStatus)) {
    return "offline";
  }
  return fallbackStatus || "offline";
}

function closedConnectionState(previous, detail) {
  return {
    ...previous,
    status: CONNECTION_STATUS.CLOSED,
    phase: null,
    statusDetail: detail || "",
    error: null,
  };
}

export function reduceConnectionState(previous = IDLE_CONNECTION_STATE, event) {
  const payload = event?.payload ?? {};
  switch (event?.type) {
    case CONNECTION_EVENT.OPEN_REQUESTED:
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTING,
        phase: CONNECTION_PHASE.CONNECTING,
        statusDetail: "",
        error: null,
      };
    case CONNECTION_EVENT.SERIAL_METADATA_RECEIVED:
      return {
        ...previous,
        detectedBaudRate: payload.baudRate || null,
        detectedBaudConfirmed: payload.confirmed === true,
        detectedSerialPort: payload.serialPort || "",
        serialScores: Array.isArray(payload.serialScores) ? payload.serialScores : [],
        serialBaudError: null,
      };
    case CONNECTION_EVENT.OPEN_FAILED:
      if (isFinalLifecycleState(previous)) return previous;
      return {
        ...previous,
        status: CONNECTION_STATUS.FAILED,
        phase: null,
        statusDetail: diagnosticDetail(payload.error, payload.detail),
        error: payload.error ?? null,
      };
    case CONNECTION_EVENT.CLOSE_REQUESTED:
      return {
        ...previous,
        status: CONNECTION_STATUS.DISCONNECTING,
        phase: CONNECTION_PHASE.DISCONNECTING,
        error: null,
      };
    case CONNECTION_EVENT.HOST_KEY_CANCELLED:
      return {
        ...previous,
        status: CONNECTION_STATUS.FAILED,
        phase: null,
        error: {
          code: "host_key_error",
          title: "",
          message: "",
          detail: "",
        },
      };
    case CONNECTION_EVENT.HOST_KEY_AUTH_FAILED:
      return {
        ...previous,
        status: CONNECTION_STATUS.FAILED,
        phase: null,
        error: payload.error ?? null,
      };
    case CONNECTION_EVENT.HOST_KEY_CHALLENGE:
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTING,
        phase: CONNECTION_PHASE.HOST_KEY_CHALLENGE,
        statusDetail: "",
        error: null,
      };
    case CONNECTION_EVENT.SESSION_READY:
      if (isFinalLifecycleState(previous)) return previous;
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTED,
        phase: null,
        statusDetail: "",
        error: null,
      };
    case CONNECTION_EVENT.SESSION_FAILED:
      if (isFinalLifecycleState(previous)) return previous;
      return {
        ...previous,
        status: CONNECTION_STATUS.FAILED,
        phase: null,
        statusDetail: diagnosticDetail(payload.error, payload.detail),
        error: previous?.error ??
          payload.error ?? {
            code: "session_failed",
            title: "",
            message: "",
            detail: payload.detail || "",
          },
      };
    case CONNECTION_EVENT.SESSION_CLOSED:
      if (isFinalLifecycleState(previous)) return previous;
      return closedConnectionState(previous, payload.detail);
    case CONNECTION_EVENT.SERIAL_REDETECT_REQUESTED:
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTED,
        phase: CONNECTION_PHASE.SERIAL_BAUD_DETECTION,
        error: null,
        serialBaudError: null,
      };
    case CONNECTION_EVENT.SERIAL_REDETECT_SUCCEEDED:
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTED,
        phase: null,
        detectedBaudRate: payload.baudRate || null,
        detectedBaudConfirmed: payload.confirmed === true,
        detectedSerialPort: payload.serialPort || "",
        serialScores: Array.isArray(payload.serialScores) ? payload.serialScores : [],
        serialBaudError: null,
      };
    case CONNECTION_EVENT.SERIAL_REDETECT_FAILED:
      return {
        ...previous,
        status: CONNECTION_STATUS.CONNECTED,
        phase: null,
        serialBaudError: payload.errorMessage || "",
      };
    default:
      return previous;
  }
}

export function connectionEventForSessionStatus(status, detail = "") {
  switch (status) {
    case "ready":
      return { type: CONNECTION_EVENT.SESSION_READY };
    case "failed":
      return {
        type: CONNECTION_EVENT.SESSION_FAILED,
        payload: {
          detail,
          error: {
            code: "session_failed",
            title: "",
            message: "",
            detail,
            recoverable: false,
          },
        },
      };
    case "closed":
      return {
        type: CONNECTION_EVENT.SESSION_CLOSED,
        payload: { detail },
      };
    default:
      return null;
  }
}
