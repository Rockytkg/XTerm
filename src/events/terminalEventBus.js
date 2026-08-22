import { listen } from "@tauri-apps/api/event";
import { createEventBus } from "../utils/createEventBus";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.terminal.events");

export const TERMINAL_EVENTS = Object.freeze({
  CONNECTION_HOST_KEY_CHALLENGE: "connection.host_key_challenge",
  SESSION_STATUS_CHANGED: "session.status_changed",
  SESSION_WORKING_DIRECTORY: "session.working_directory",
  METRICS_REPORT: "metrics.report",
  SFTP_TRANSFER_PROGRESS: "sftp.transfer_progress",
});

const terminalEventBus = createEventBus({ logger });

let bridgeStartPromise = null;
let bridgeStopFns = [];
/** Number of consumers (observation subscriptions) the bridge feeds. */
let bridgeConsumers = 0;

async function startTerminalEventBridge() {
  if (bridgeStartPromise) return bridgeStartPromise;
  logger.info("bridge.start", {
    consumerCount: bridgeConsumers,
  });
  bridgeStartPromise = listen("terminal-event", (event) => {
    const name = event?.payload?.name;
    if (!name) return;
    terminalEventBus.emit(name, event.payload.payload);
  })
    .then((unlisten) => {
      bridgeStopFns.push(unlisten);
      logger.info("bridge.started", {
        consumerCount: bridgeConsumers,
      });
    })
    .catch((error) => {
      bridgeStartPromise = null;
      while (bridgeStopFns.length > 0) {
        const unlisten = bridgeStopFns.pop();
        try {
          unlisten?.();
        } catch (stopError) {
          logger.error("terminalEventBridge cleanup failed:", stopError);
        }
      }
      throw error;
    });
  return bridgeStartPromise;
}

function stopTerminalEventBridge() {
  if (bridgeConsumers > 0) return;
  logger.info("bridge.stop");
  while (bridgeStopFns.length > 0) {
    const unlisten = bridgeStopFns.pop();
    try {
      unlisten?.();
    } catch (error) {
      logger.error("terminalEventBridge unsubscribe failed:", error);
    }
  }
  bridgeStartPromise = null;
}

/** Retain the native bridge for one observation subscription. */
async function retainBridge() {
  bridgeConsumers += 1;
  try {
    await startTerminalEventBridge();
  } catch (error) {
    bridgeConsumers = Math.max(0, bridgeConsumers - 1);
    throw error;
  }
  return () => {
    bridgeConsumers = Math.max(0, bridgeConsumers - 1);
    if (bridgeConsumers === 0) {
      stopTerminalEventBridge();
    }
  };
}

export async function observeTerminalEvent(type, handler) {
  // Use a per-subscription wrapper so registering the same handler twice does
  // not let one disposer remove the other subscription from the Set.
  const unlisten = terminalEventBus.on(type, (payload) => handler(payload));
  try {
    const release = await retainBridge();
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      unlisten();
      release();
    };
  } catch (error) {
    unlisten();
    throw error;
  }
}
