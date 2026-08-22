import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const preferencesLogger = createLogger("frontend.preferences.service");

const runPreferenceRequest = createServiceRunner({
  logger: preferencesLogger,
  module: "preferences",
});

export function getPreferences() {
  return runPreferenceRequest("preferences_get", undefined, {
    action: "preferences.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function getSetting(key) {
  return runPreferenceRequest(
    "setting_get",
    { key },
    {
      action: "preferences.setting_get",
      level: "debug",
      successLevel: "debug",
      context: { settingKey: key },
    },
  );
}

export function setPreference(key, value) {
  return runPreferenceRequest(
    "setting_set",
    { key, value },
    {
      action: "preferences.setting_set",
      context: { settingKey: key },
      summarizePayload: () => ({
        key,
        value,
      }),
    },
  );
}

export function resetPreferencesStore() {
  return runPreferenceRequest("preferences_reset", undefined, {
    action: "preferences.reset",
    level: "warn",
    successLevel: "info",
  });
}
