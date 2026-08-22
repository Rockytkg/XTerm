import { listen } from "@tauri-apps/api/event";
import { createLogger } from "../utils/logger";
import { createEventBridge } from "../utils/eventBridge";

const logger = createLogger("frontend.proxy.events");
export const observeProxyStats = createEventBridge({
  eventName: "proxy-stats",
  logName: "proxy.stats",
  logger,
  subscribe: listen,
});
