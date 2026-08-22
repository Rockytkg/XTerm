import { invokeIpc } from "./ipc/core";

export function getAppMetadata() {
  return invokeIpc("app_metadata");
}

export function checkForUpdates() {
  return invokeIpc("check_for_updates");
}

export function openExternalUrl(url) {
  return invokeIpc("plugin:opener|open_url", { url });
}

export function restartApp() {
  return invokeIpc("app_restart");
}
