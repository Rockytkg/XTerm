import { createLogger, setGlobalLogLevel } from "../utils/logger";
import { createServiceRunner } from "./ipc/serviceRunner";

const loggingLogger = createLogger("frontend.logging.service");

const runLoggingRequest = createServiceRunner({
  logger: loggingLogger,
  module: "logging",
});

/** 拉取后端持久化的日志级别并同步到前端全局门控，返回生效级别。 */
export async function getLogLevel() {
  const level = await runLoggingRequest("log_level_get", undefined, {
    action: "log-level.get",
    level: "debug",
    successLevel: "debug",
  });
  return setGlobalLogLevel(level);
}

/** 更新后端日志级别（立即生效并持久化），同步前端全局门控。 */
export async function setLogLevel(level) {
  const nextLevel = await runLoggingRequest(
    "log_level_set",
    { level },
    { action: "log-level.set" },
  );
  return setGlobalLogLevel(nextLevel);
}

export function listLogFiles() {
  return runLoggingRequest("log_files_list", undefined, {
    action: "log-files.list",
    level: "debug",
    successLevel: "debug",
  });
}

export function readLogTail(name, maxBytes) {
  return runLoggingRequest(
    "log_file_tail",
    { name, maxBytes },
    {
      action: "log-file.tail",
      level: "debug",
      successLevel: "debug",
      summarizePayload: () => ({ name, maxBytes }),
    },
  );
}

export function pruneLogFiles() {
  return runLoggingRequest("log_files_prune", undefined, {
    action: "log-files.prune",
  });
}

export function openLogDir() {
  return runLoggingRequest("log_dir_open", undefined, {
    action: "log-dir.open",
  });
}
