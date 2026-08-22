import { bytesToUtf8, decodeBuffer, encodeBuffer } from "./bytes";

export class TransferError extends Error {
  constructor(message, type = null, trace = false) {
    if (type === "fail" || type === "FAIL" || type === "EXIT") {
      try {
        message = bytesToUtf8(decodeBuffer(message));
      } catch (error) {
        message = `decode [${message}] error: ${error}`;
      }
    } else if (type) {
      message = `[TrzszError] ${type}: ${message}`;
    }
    super(message);
    this.name = "TrzszError";
    this.type = type;
    this.trace = trace;
  }

  isTraceBack() {
    return this.type !== "fail" && this.type !== "EXIT" && this.trace;
  }

  isRemoteExit() {
    return this.type === "EXIT";
  }

  isRemoteFail() {
    return this.type === "fail" || this.type === "FAIL";
  }

  isStopAndDelete() {
    return this.type === "fail" && this.message === "Stopped and deleted";
  }

  static message(error) {
    if (error instanceof TransferError && !error.isTraceBack()) return error.message;
    return error?.stack ? error.stack.replace("TrzszError: ", "") : String(error);
  }
}

export function normalizeError(error) {
  if (error instanceof Error) return error;
  return new Error(String(error));
}

export function protocolParseError(line) {
  return new TransferError(encodeBuffer(line), "colon", true);
}
