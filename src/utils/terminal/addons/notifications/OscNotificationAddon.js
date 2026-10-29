import {
  sendNotification,
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

const OSC_9_PROGRESS_PREFIX = "4;";
// 远端程序可以无限发 OSC 9/777：不限速会让一次输出风暴弹满桌面通知。
// 每终端每秒最多一条，超出的直接丢弃；正文长度同样设限（单条 OSC 载荷
// 上限约 10MB，不能原样塞进系统通知）。
const MIN_NOTIFICATION_INTERVAL_MS = 1000;
const MAX_TITLE_CHARS = 100;
const MAX_BODY_CHARS = 500;

let notificationPermissionResolved = false;
let notificationPermissionGranted = false;
let notificationPermissionRequest = null;

async function ensureNotificationPermission() {
  if (notificationPermissionResolved) return notificationPermissionGranted;
  if (!notificationPermissionRequest) {
    notificationPermissionRequest = (async () => {
      try {
        notificationPermissionGranted = await isPermissionGranted();
        if (!notificationPermissionGranted) {
          notificationPermissionGranted = (await requestPermission()) === "granted";
        }
      } catch {
        notificationPermissionGranted = false;
      } finally {
        notificationPermissionResolved = true;
      }
      return notificationPermissionGranted;
    })();
  }
  return notificationPermissionRequest;
}

function normalizeNotificationText(value) {
  return String(value ?? "")
    .replace(/\s+/gu, " ")
    .trim();
}

function truncateText(value, maxChars) {
  return value.length > maxChars ? `${value.slice(0, maxChars)}…` : value;
}

async function notifyDesktop({ title, body }) {
  if (!(await ensureNotificationPermission())) return false;
  title = truncateText(normalizeNotificationText(title), MAX_TITLE_CHARS);
  body = truncateText(normalizeNotificationText(body), MAX_BODY_CHARS);
  if (!title && !body) return false;
  await sendNotification(
    title ? { title, body: body || title } : { title: "Terminal notification", body },
  );
  return true;
}

export class OscNotificationAddon {
  constructor() {
    this._handlers = [];
    this._lastNotificationAt = 0;
  }

  activate(terminal) {
    this._handlers.push(
      terminal.parser.registerOscHandler(9, (data) => {
        if (!data || data.startsWith(OSC_9_PROGRESS_PREFIX)) return false;
        this._notify({ body: data });
        return true;
      }),
    );
    this._handlers.push(
      terminal.parser.registerOscHandler(777, (data) => {
        const [command, title = "", ...rest] = String(data || "").split(";");
        if (command !== "notify") return false;
        this._notify({ title, body: rest.join(";") });
        return true;
      }),
    );
  }

  _notify(details) {
    const now = Date.now();
    if (now - this._lastNotificationAt < MIN_NOTIFICATION_INTERVAL_MS) return;
    this._lastNotificationAt = now;
    void notifyDesktop(details).catch(() => {});
  }

  dispose() {
    for (const handler of this._handlers) handler?.dispose?.();
    this._handlers = [];
  }
}
