import {
  readText as readTauriClipboardText,
  writeText as writeTauriClipboardText,
} from "@tauri-apps/plugin-clipboard-manager";
import { createLogger } from "../../../logger";

const logger = createLogger("frontend.terminal.clipboard-addon");

function stripBase64Padding(value) {
  return String(value || "").replace(/=+$/u, "");
}

function encodeText(data) {
  const bytes = new TextEncoder().encode(String(data ?? ""));
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}

function decodeText(data) {
  try {
    const normalized = String(data || "").replace(/\s+/gu, "");
    const binary = atob(normalized);
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    const text = new TextDecoder().decode(bytes);
    if (stripBase64Padding(encodeText(text)) !== stripBase64Padding(normalized)) {
      return "";
    }
    return text;
  } catch {
    return "";
  }
}

export class TauriClipboardAddon {
  constructor() {
    this._terminal = null;
    this._oscHandler = null;
  }

  activate(terminal) {
    this._terminal = terminal;
    this._oscHandler = terminal.parser.registerOscHandler(52, (data) =>
      this._setOrReportClipboard(data),
    );
  }

  dispose() {
    this._oscHandler?.dispose?.();
    this._oscHandler = null;
    this._terminal = null;
  }

  isSupported() {
    return true;
  }

  async copySelection() {
    const text = this._terminal?.getSelection();
    if (!text) return false;
    await writeTauriClipboardText(text);
    this._terminal.clearSelection();
    return true;
  }

  async pasteIntoTerminal() {
    if (!this._terminal) return false;
    const text = await readTauriClipboardText();
    if (!text) return false;
    this._terminal.paste(text);
    this._terminal.focus();
    return true;
  }

  _reportClipboard(selection, data) {
    const payload = encodeText(data);
    this._terminal?.input(`\u001b]52;${selection};${payload}\u0007`, false);
  }

  _setOrReportClipboard(data) {
    const [selection = "", payload = ""] = String(data || "").split(";");
    if (!selection && !payload) return true;

    if (payload === "?") {
      return readTauriClipboardText()
        .then((text) => (this._reportClipboard(selection, selection === "c" ? text : ""), true))
        .catch((error) => (logger.warn("osc52.clipboard.read.failed", error), true));
    }

    if (selection !== "c") return true;
    return writeTauriClipboardText(decodeText(payload))
      .then(() => true)
      .catch((error) => (logger.warn("osc52.clipboard.write.failed", error), true));
  }
}
