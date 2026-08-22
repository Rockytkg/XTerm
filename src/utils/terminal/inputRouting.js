import { BACKSPACE_SENDS } from "../terminalSessionOptions";
import { bytesToBase64, stringToBytes } from "./addons/trzsz/bytes";

export function binaryStringToBase64(value) {
  return bytesToBase64(stringToBytes(String(value || "")));
}

export function shouldSendBackspaceAsBs(event, connection, sessionId) {
  return (
    event.type === "keydown" &&
    event.key === "Backspace" &&
    !!sessionId &&
    connection?.backspaceSends === BACKSPACE_SENDS.BS &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.metaKey
  );
}
