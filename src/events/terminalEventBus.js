import { listen } from "@tauri-apps/api/event";
import { createEventBridge } from "../utils/eventBridge";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.terminal.events");

export const TERMINAL_EVENTS = Object.freeze({
  CONNECTION_HOST_KEY_CHALLENGE: "connection.host_key_challenge",
  SESSION_STATUS_CHANGED: "session.status_changed",
  SESSION_WORKING_DIRECTORY: "session.working_directory",
  METRICS_REPORT: "metrics.report",
  SFTP_TRANSFER_PROGRESS: "sftp.transfer_progress",
});

// 原生桥的生命周期（注册先行防观察间隙、start in-flight 去重、末订阅释放时
// 停桥）由共享的 createEventBridge 承担；这里只保留按事件名过滤的薄封装与
// 生命周期日志。
const observeTerminalEnvelope = createEventBridge({
  eventName: "terminal-event",
  logName: "terminalEventBridge",
  logger,
  subscribe: listen,
  onStart: (consumerCount) => logger.info("bridge.start", { consumerCount }),
  onStarted: (consumerCount) => logger.info("bridge.started", { consumerCount }),
  onStop: () => logger.info("bridge.stop"),
});

export async function observeTerminalEvent(type, handler) {
  // 监听器异常就地隔离并沿用原事件总线的日志文案，不向 dispatch 外抛。
  return observeTerminalEnvelope((envelope) => {
    if (envelope?.name !== type) return;
    try {
      handler(envelope.payload);
    } catch (error) {
      logger.error("eventBus listener failed:", type, error);
    }
  });
}
