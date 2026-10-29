import { Channel, invoke } from "@tauri-apps/api/core";
import { invokeDetailedIpc } from "../../../services/ipc/core";
import { TERMINAL_FRAME_TYPES, normalizeOutputFrame } from "../protocol/frames";
import { createTerminalChannelLease, leaseKey } from "../sessionIdentity";

class TauriTerminalTransport {
  constructor({ logger } = {}) {
    this.logger = logger;
    this.outputSubscriptions = new Map();
  }

  async attach(sessionId, handler) {
    const channel = new Channel();
    channel.onmessage = (payload) => {
      handler(normalizeOutputFrame(payload));
    };
    try {
      const response = await invokeDetailedIpc(
        "terminal_attach",
        { sessionId, channel },
        {
          scope: this.logger,
          level: "debug",
          successLevel: "debug",
          summarizePayload: () => ({ sessionId, channel: "tauri-channel" }),
          summarizeResult: (value) => ({
            connectionId: value?.connectionId,
            sessionId: value?.sessionId,
            channelId: value?.channelId,
            subscriptionId: value?.subscriptionId,
            alreadyActive: value?.alreadyActive,
          }),
        },
      );
      const lease = createTerminalChannelLease({
        alreadyActive: response?.alreadyActive,
        channelId: response?.channelId,
        connectionId: response?.connectionId,
        sessionId: response?.sessionId || sessionId,
        subscriptionId: response?.subscriptionId,
      });
      const subscription = {
        channel,
        lease,
      };
      this.outputSubscriptions.set(leaseKey(lease), subscription);
      return lease;
    } catch (error) {
      channel.onmessage = null;
      throw error;
    }
  }

  async detach(sessionId, channelId = null) {
    const subscription = this.findSubscription(sessionId, channelId);
    try {
      await this.invokeDetach({
        sessionId,
        channelId,
        subscriptionId: subscription?.lease?.subscriptionId ?? null,
      });
    } finally {
      if (subscription) {
        this.detachOutputSubscription(subscription);
      }
    }
  }

  invokeDetach({ sessionId, channelId = null, subscriptionId = null }) {
    return invokeDetailedIpc(
      "terminal_detach",
      {
        request: {
          sessionId,
          channelId,
          subscriptionId,
        },
      },
      {
        scope: this.logger,
        level: "debug",
        successLevel: "debug",
        failureLevel: "warn",
        summarizePayload: () => ({ sessionId, channelId, subscriptionId }),
      },
    );
  }

  findSubscription(sessionId, channelId = null) {
    for (const subscription of this.outputSubscriptions.values()) {
      if (subscription.lease.sessionId !== sessionId) continue;
      if (channelId !== null && subscription.lease.channelId !== channelId) continue;
      return subscription;
    }
    return null;
  }

  detachOutputSubscription(subscription) {
    if (!subscription) return;
    subscription.channel.onmessage = null;
    this.outputSubscriptions.delete(leaseKey(subscription.lease));
  }

  /**
   * 本地丢弃输出订阅，不发 IPC。用于后端连接已失败/关闭、或 attach 结果
   * 被判 stale 等无需（或不应）再通知后端的场景，避免 outputSubscriptions
   * 残留到组件卸载才回收。
   */
  dropSubscription(sessionId, channelId = null) {
    const subscription = this.findSubscription(sessionId, channelId);
    if (subscription) {
      this.detachOutputSubscription(subscription);
    }
  }

  /**
   * 输入帧实时直发：一帧一次 invoke，不做微任务合帧/攒批。顺序由两点保证——
   * webview IPC 消息按发起顺序投递，且 terminal_send_batch 是同步命令、在主
   * 线程按投递顺序就地执行（与输出方向 Channel 消息保序同一前提）；帧内的
   * inputSequence 作为单调去重守卫，丢弃 channel 失配或陈旧的帧。
   * 热路径有意使用裸 invoke：终端输入帧频率高，invokeDetailedIpc 的逐条日志
   * 开销不可忽略；失败由调用方捕获并记录。
   */
  sendFrame(frame) {
    return invoke("terminal_send_batch", { frames: [frame] });
  }

  send(frame) {
    switch (frame?.type) {
      case TERMINAL_FRAME_TYPES.ATTACH:
        return this.attach(frame.sessionId, frame.onOutput);
      case TERMINAL_FRAME_TYPES.DETACH:
        return this.detach(frame.sessionId, frame.channelId);
      case TERMINAL_FRAME_TYPES.INPUT_TEXT:
      case TERMINAL_FRAME_TYPES.INPUT_BYTES:
      case TERMINAL_FRAME_TYPES.RESIZE:
      case TERMINAL_FRAME_TYPES.RAW_OUTPUT:
        return this.sendFrame(frame);
      default:
        this.logger?.warn("Unsupported terminal transport frame", frame);
        return Promise.reject(new Error(`Unsupported terminal transport frame '${frame?.type}'`));
    }
  }
}

export function createTauriTerminalTransport(options = {}) {
  return new TauriTerminalTransport(options);
}
