import { createLogger } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const fileServiceLogger = createLogger("frontend.file_service.service");

const runFileServiceRequest = createServiceRunner({
  logger: fileServiceLogger,
  module: "file_service",
});

export function getFileServiceConfig() {
  return runFileServiceRequest("get_config", undefined, {
    action: "file_service.config.get",
    level: "debug",
    successLevel: "debug",
  });
}

export function chooseSharedDirectory(defaultPath = "", title = "") {
  return runFileServiceRequest(
    "choose_shared_directory",
    { request: { defaultPath, title } },
    {
      action: "file_service.shared_dir.choose",
      level: "debug",
      successLevel: "debug",
    },
  );
}

export function startFileService({ protocol, bindIp, sharedDir }) {
  return runFileServiceRequest(
    "start_file_service",
    { protocol, bindIp, sharedDir },
    {
      action: `file_service.${protocol}.start`,
      context: { protocol, bindIp },
      summarizePayload: () => ({ protocol, bindIp, sharedDir }),
    },
  );
}

export function stopFileService() {
  return runFileServiceRequest("stop", undefined, {
    action: "file_service.stop",
    level: "warn",
    successLevel: "info",
  });
}

export function setFileServiceBindIp(bindIp) {
  return runFileServiceRequest(
    "set_bind_ip",
    { bindIp },
    {
      action: "file_service.bind_ip.set",
      context: { bindIp },
      summarizePayload: () => ({ bindIp }),
    },
  );
}

export function setFileServiceSharedDir(sharedDir) {
  return runFileServiceRequest(
    "set_shared_dir",
    { sharedDir },
    {
      action: "file_service.shared_dir.set",
      summarizePayload: () => ({ sharedDir }),
    },
  );
}

export function setFileServiceCredentials(username) {
  return runFileServiceRequest(
    "set_credentials",
    { username },
    {
      action: "file_service.credentials.set",
      summarizePayload: () => ({ username }),
    },
  );
}

export function setFileServicePassword(password) {
  return runFileServiceRequest(
    "file_service_set_password",
    { password },
    {
      action: "file_service.password.set",
      // 口令本身不进日志，只记录是否非空。
      summarizePayload: () => ({ passwordConfigured: Boolean(password) }),
    },
  );
}
