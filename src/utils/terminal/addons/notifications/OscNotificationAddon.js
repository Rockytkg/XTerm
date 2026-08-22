import {
  sendNotification,
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

const OSC_9_PROGRESS_PREFIX = "4;";

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

async function notifyDesktop({ title, body }) {
  if (!(await ensureNotificationPermission())) return false;
  title = normalizeNotificationText(title);
  body = normalizeNotificationText(body);
  if (!title && !body) return false;
  await sendNotification(
    title ? { title, body: body || title } : { title: "Terminal notification", body },
  );
  return true;
}

export class OscNotificationAddon {
  constructor() {
    this._handlers = [];
  }

  activate(terminal) {
    this._handlers.push(
      terminal.parser.registerOscHandler(9, (data) => {
        if (!data || data.startsWith(OSC_9_PROGRESS_PREFIX)) return false;
        void notifyDesktop({ body: data });
        return true;
      }),
    );
    this._handlers.push(
      terminal.parser.registerOscHandler(777, (data) => {
        const [command, title = "", ...rest] = String(data || "").split(";");
        if (command !== "notify") return false;
        void notifyDesktop({ title, body: rest.join(";") });
        return true;
      }),
    );
  }

  dispose() {
    for (const handler of this._handlers) handler?.dispose?.();
    this._handlers = [];
  }
}
