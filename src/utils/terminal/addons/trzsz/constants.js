export const DEFAULT_MAX_CHUNK_SIZE = 10 * 1024 * 1024;
export const TRZSZ_VERSION = "1.1.6";
export const TRZSZ_TRIGGER = "::TRZSZ:TRANSFER:";
export const TRZSZ_TRIGGER_PATTERN = /::TRZSZ:TRANSFER:([SRD]):(\d+\.\d+\.\d+)(:\d+)?/u;
export const DRAG_INIT_TIMEOUT = 3000;
export const EMPTY_MD5 = Uint8Array.from([
  0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8, 0x42, 0x7e,
]);
