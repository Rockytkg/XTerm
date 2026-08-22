import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const pathSettingsLogger = createLogger("frontend.paths.service");

const runPathRequest = createServiceRunner({
  logger: pathSettingsLogger,
  module: "paths",
});

export function getPathSettings() {
  return runPathRequest("path_settings_get", undefined, {
    action: "path_settings.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function setPathSettings(settings) {
  return runPathRequest(
    "path_settings_set",
    { settings },
    {
      action: "path_settings.set",
      summarizePayload: () => ({
        dataDir: settings?.dataDir,
        logsDir: settings?.logsDir,
      }),
    },
  );
}

export function chooseDirectory(defaultPath = "", title = "") {
  return runPathRequest(
    "path_settings_choose_directory",
    {
      request: {
        defaultPath,
        title,
      },
    },
    {
      action: "path_settings.choose_directory",
      summarizePayload: () => ({
        defaultPath,
        title,
      }),
    },
  );
}
