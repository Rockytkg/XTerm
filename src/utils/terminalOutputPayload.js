const utf8Encoder = new TextEncoder();

function isTerminalOutputChannelAccepted(payload, state = {}) {
  const payloadChannelId = payload?.channelId;
  if (!Number.isSafeInteger(payloadChannelId)) return false;
  if (state.sessionChannelId && payloadChannelId === state.sessionChannelId) return true;
  return (
    !!state.closingSessionId &&
    state.closingSessionId === state.sessionId &&
    state.closingSessionChannelId === payloadChannelId
  );
}

function cursorState(state) {
  const sameSession = state.terminalOutputCursorSessionId === state.sessionId;
  return {
    cursor: sameSession ? state.terminalOutputCursor : 0,
    sameSession,
  };
}

function outputRange(payload) {
  const startOffset = payload?.startOffset;
  const endOffset = payload?.endOffset;
  if (
    !Number.isSafeInteger(startOffset) ||
    !Number.isSafeInteger(endOffset) ||
    startOffset < 0 ||
    endOffset < startOffset
  ) {
    return null;
  }
  return { startOffset, endOffset };
}

function isStaleRange(range, state) {
  if (!range) return false;
  const { cursor, sameSession } = cursorState(state);
  return sameSession && range.endOffset <= cursor;
}

const BASE64_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_LOOKUP = new Uint8Array(256).fill(255);
for (let i = 0; i < BASE64_CHARS.length; i += 1) {
  BASE64_LOOKUP[BASE64_CHARS.charCodeAt(i)] = i;
}

function decodeBase64Bytes(dataBase64) {
  const input = String(dataBase64 || "");
  const inputLength = input.length;
  if (inputLength === 0) return new Uint8Array(0);

  const maxOutputLength = Math.floor((inputLength * 3) / 4);
  const output = new Uint8Array(maxOutputLength);
  let buffer = 0;
  let bits = 0;
  let written = 0;

  for (let i = 0; i < inputLength; i += 1) {
    const code = input.charCodeAt(i);
    if (code === 0x3d) break; // '=' padding
    const value = BASE64_LOOKUP[code];
    if (value === 255) continue; // ignore invalid/whitespace characters
    buffer = (buffer << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      output[written] = (buffer >> bits) & 0xff;
      written += 1;
    }
  }

  return written === maxOutputLength ? output : output.subarray(0, written);
}

function encodeBase64Bytes(bytes) {
  let raw = "";
  for (const byte of bytes) raw += String.fromCharCode(byte);
  return btoa(raw);
}

function createTextDecoder(encoding) {
  try {
    return new TextDecoder(encoding || "utf-8");
  } catch {
    return new TextDecoder();
  }
}

export function createTerminalOutputByteDecoder() {
  let decoder = null;
  let activeEncoding = "";

  return {
    decode(dataBase64, encoding = "utf-8") {
      const bytes = decodeBase64Bytes(dataBase64);
      if (!bytes) return "";
      const nextEncoding = String(encoding || "utf-8").toLowerCase();
      if (!decoder || activeEncoding !== nextEncoding) {
        decoder = createTextDecoder(nextEncoding);
        activeEncoding = nextEncoding;
      }
      return decoder.decode(bytes, { stream: true });
    },
    reset() {
      decoder = null;
      activeEncoding = "";
    },
  };
}

function sliceUtf8ByByteOffset(text, byteOffset) {
  if (byteOffset <= 0 || !text) return text;
  let consumed = 0;
  let codeUnitIndex = 0;
  for (const char of text) {
    const next = consumed + utf8Encoder.encode(char).length;
    if (next > byteOffset) {
      return text.slice(codeUnitIndex);
    }
    consumed = next;
    codeUnitIndex += char.length;
  }
  return "";
}

function normalizeTextOutput(rawData, range, state) {
  if (!range) return null;
  if (isStaleRange(range, state)) return null;
  const { cursor, sameSession } = cursorState(state);
  const trimBytes = sameSession ? Math.max(0, cursor - range.startOffset) : 0;
  return {
    data: sliceUtf8ByByteOffset(rawData, trimBytes),
    outputKind: "text",
    endOffset: range.endOffset,
  };
}

function normalizeBytesOutput(rawBase64, range, state) {
  if (!range || isStaleRange(range, state)) return null;
  const { cursor, sameSession } = cursorState(state);
  const trimBytes = sameSession ? Math.max(0, cursor - range.startOffset) : 0;
  if (trimBytes <= 0) {
    // 无裁剪时直接透传原始 base64：下游的字节解码器会做唯一一次解码，
    // 避免 decode→encode→decode 的三重编解码。非法 base64 由下游解码失败兜底。
    return {
      dataBase64: rawBase64,
      outputKind: "bytes",
      endOffset: range.endOffset,
    };
  }
  const bytes = decodeBase64Bytes(rawBase64);
  if (!bytes) return null;
  const visibleBytes = bytes.subarray(Math.min(trimBytes, bytes.length));
  return {
    dataBase64: encodeBase64Bytes(visibleBytes),
    outputKind: "bytes",
    endOffset: range.endOffset,
  };
}

function normalizeTerminalOutputPayload(payload, state = {}) {
  const payloadKind = payload?.kind;
  const rawData = payloadKind === "text" && typeof payload.data === "string" ? payload.data : "";
  const rawBase64 =
    payloadKind === "bytes" && typeof payload.dataBase64 === "string" ? payload.dataBase64 : "";
  if (!rawData && !rawBase64) return null;

  if (!isTerminalOutputChannelAccepted(payload, state)) {
    return null;
  }

  if (!rawData && rawBase64) {
    return normalizeBytesOutput(rawBase64, outputRange(payload), state);
  }

  return normalizeTextOutput(rawData, outputRange(payload), state);
}

export function classifyTerminalOutputPayload(payload, state = {}) {
  const normalized = normalizeTerminalOutputPayload(payload, state);
  if (!normalized?.data && !normalized?.dataBase64) {
    return { kind: "ignore", normalized: null };
  }
  if (normalized.outputKind === "bytes") {
    return { kind: "raw", normalized };
  }
  return { kind: "text", normalized };
}
