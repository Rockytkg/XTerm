import pako from "pako";

const BASE64_CHUNK_SIZE = 0x8000;
const TEXT_DECODER = new TextDecoder();
const TEXT_ENCODER = new TextEncoder();

export function stringToBytes(value) {
  const bytes = new Uint8Array(value.length);
  for (let index = 0; index < value.length; index += 1) {
    bytes[index] = value.charCodeAt(index) & 0xff;
  }
  return bytes;
}

export function bytesToBinaryString(bytes) {
  let text = "";
  for (let index = 0; index < bytes.length; index += BASE64_CHUNK_SIZE) {
    text += String.fromCharCode(...bytes.subarray(index, index + BASE64_CHUNK_SIZE));
  }
  return text;
}

export function bytesToBase64(bytes, { stripPadding = true } = {}) {
  const base64 = btoa(bytesToBinaryString(bytes));
  return stripPadding ? base64.replace(/=+$/u, "") : base64;
}

export function base64ToBytes(value) {
  const raw = String(value || "");
  if (!raw) return new Uint8Array();
  const padded = raw.padEnd(Math.ceil(raw.length / 4) * 4, "=");
  return stringToBytes(atob(padded));
}

export function bytesToUtf8(bytes) {
  return TEXT_DECODER.decode(bytes);
}

function utf8ToBytes(value) {
  return TEXT_ENCODER.encode(value);
}

export function asBytes(value) {
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  if (Array.isArray(value)) return Uint8Array.from(value);
  if (value?.type === "write") return asBytes(value.data);
  if (typeof value === "string") return stringToBytes(value);
  return new Uint8Array();
}

export function payloadBytes(payload) {
  const raw = String(payload?.dataBase64 || "");
  if (!raw) return stringToBytes(String(payload?.data || ""));
  return base64ToBytes(raw);
}

export function bytesIncludeAscii(bytes, ascii) {
  if (!bytes?.length || !ascii) return false;
  const pattern = stringToBytes(ascii);
  outer: for (let start = 0; start <= bytes.length - pattern.length; start += 1) {
    for (let offset = 0; offset < pattern.length; offset += 1) {
      if (bytes[start + offset] !== pattern[offset]) continue outer;
    }
    return true;
  }
  return false;
}

export function encodeBuffer(value) {
  const input = typeof value === "string" ? utf8ToBytes(value) : asBytes(value);
  return bytesToBase64(pako.deflate(input), { stripPadding: false });
}

export function decodeBuffer(value) {
  return pako.inflate(base64ToBytes(value));
}

export function bytesEqual(left, right) {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return false;
  }
  return true;
}
