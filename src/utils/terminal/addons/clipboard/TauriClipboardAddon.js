import {
  readText as readTauriClipboardText,
  writeText as writeTauriClipboardText,
} from "@tauri-apps/plugin-clipboard-manager";
import { createLogger } from "../../../logger";

const logger = createLogger("frontend.terminal.clipboard-addon");

// OSC 52 写入的应用层上限：解析器层允许约 10MB 的 base64，但剪贴板写入远超
// 实用规模，超限内容直接丢弃，避免远端用大载荷覆盖本地剪贴板。
const MAX_OSC52_WRITE_BYTES = 4 * 1024 * 1024;

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
      // OSC 52 读取默认拒绝：远端程序无需任何确认即可取走用户剪贴板，
      // kitty / Windows Terminal 等终端同样默认禁止该方向。回空串让协议
      // 正常收尾，同时留日志便于排查依赖该能力的程序。
      logger.warn("osc52.clipboard.read.denied", { selection });
      this._reportClipboard(selection, "");
      return true;
    }

    if (selection !== "c") return true;
    // 解码前按 base64 长度估算解码后大小，避免先分配再拒绝。
    if (Math.ceil((payload.length * 3) / 4) > MAX_OSC52_WRITE_BYTES) {
      logger.warn("osc52.clipboard.write.too_large", { base64Length: payload.length });
      return true;
    }
    return writeTauriClipboardText(decodeText(payload))
      .then(() => true)
      .catch((error) => (logger.warn("osc52.clipboard.write.failed", error), true));
  }
}
