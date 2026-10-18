import { BACKSPACE_SENDS } from "../terminalSessionOptions.js";
import { bytesToBase64, stringToBytes } from "./addons/trzsz/bytes.js";

export function binaryStringToBase64(value) {
  return bytesToBase64(stringToBytes(String(value || "")));
}

export function shouldSendBackspaceAsBs(event, connection, sessionId) {
  return (
    event.type === "keydown" &&
    event.key === "Backspace" &&
    !!sessionId &&
    // 默认 BS：只有显式选择 DEL(^?) 的会话才放行为 xterm 原生 \x7f。
    // 历史 profile 里的 "AUTO" 等未知值同样落到默认 BS。
    connection?.backspaceSends !== BACKSPACE_SENDS.DEL &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey
  );
}
