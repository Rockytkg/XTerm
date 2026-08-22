import { listen } from "@tauri-apps/api/event";
import { createLogger } from "../utils/logger";
import { createEventBridge } from "../utils/eventBridge";

const logger = createLogger("frontend.file_service.events");

export const observeFileTransfers = createEventBridge({
  eventName: "file-transfer",
  logName: "file_service.transfer",
  logger,
  subscribe: listen,
});
export const observeFileServiceConfig = createEventBridge({
  eventName: "file-service-config",
  logName: "file_service.config",
  logger,
  subscribe: listen,
});
