import { asBytes, bytesToBinaryString } from "./bytes";
import { isVtSequenceEnd } from "./terminalBuffer";

export function stripTmuxStatusLine(value) {
  let text = value;
  while (true) {
    const begin = text.indexOf("\x1bP=");
    if (begin < 0) return text;
    let cursor = begin + 3;
    const nested = text.substring(cursor).indexOf("\x1bP=");
    if (nested < 0) return text.substring(0, begin);
    cursor += nested + 3;
    const end = text.substring(cursor).indexOf("\x1b\\");
    if (end < 0) return text.substring(0, begin);
    cursor += end + 2;
    text = text.substring(0, begin) + text.substring(cursor);
  }
}

export function stripServerOutput(output) {
  const bytes = asBytes(output);
  const result = new Uint8Array(bytes.length);
  let offset = 0;
  let skipEscape = false;
  for (const byte of bytes) {
    if (skipEscape) {
      if (isVtSequenceEnd(byte)) skipEscape = false;
    } else if (byte === 0x1b) {
      skipEscape = true;
    } else {
      result[offset++] = byte;
    }
  }
  while (offset > 0 && (result[offset - 1] === 0x0d || result[offset - 1] === 0x0a)) {
    offset -= 1;
  }
  if (offset > 100) return output;
  return bytesToBinaryString(result.subarray(0, offset));
}

export function formatSavedFiles(fileNames, destination) {
  let message = `Saved ${fileNames.length} ${fileNames.length > 1 ? "files/directories" : "file/directory"}`;
  if (destination) message += ` to ${destination}`;
  return [message, ...fileNames].join("\r\n- ");
}
