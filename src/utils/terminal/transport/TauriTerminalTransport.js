import { Channel, invoke } from "@tauri-apps/api/core";
import { invokeDetailedIpc } from "../../../services/ipc/core";
import { TERMINAL_FRAME_TYPES, normalizeOutputFrame } from "../protocol/frames";
import { createTerminalChannelLease, leaseKey } from "../sessionIdentity";

class TauriTerminalTransport {
  constructor({ logger } = {}) {
    this.logger = logger;
    this.outputSubscriptions = new Map();
    this.queuedBatch = null;
    this.batchChain = Promise.resolve();
  }

  async attach(sessionId, handler) {
    await this.settleQueuedBatch();
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
    await this.settleQueuedBatch();
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

  async settleQueuedBatch() {
    const batch = this.queuedBatch;
    if (!batch) return;
    try {
      await batch.promise;
    } catch (error) {
      this.logger?.warn("Pending terminal batch failed before channel transition:", error);
    }
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

  queueBatchFrame(frame) {
    if (!frame) return Promise.resolve();
    const batch = this.queuedBatch ?? this.createQueuedBatch();
    batch.frames.push(frame);
    if (!batch.scheduled) {
      batch.scheduled = true;
      queueMicrotask(() => this.flushQueuedBatch(batch));
    }
    return batch.promise;
  }

  createQueuedBatch() {
    const batch = { frames: [], scheduled: false };
    batch.promise = new Promise((resolve, reject) => {
      batch.resolve = resolve;
      batch.reject = reject;
    });
    this.queuedBatch = batch;
    return batch;
  }

  async flushQueuedBatch(batch) {
    if (this.queuedBatch === batch) this.queuedBatch = null;

    try {
      const send = () => this.sendBatch(batch.frames);
      const sendPromise = this.batchChain.then(send, send);
      this.batchChain = sendPromise.catch(() => {});
      await sendPromise;
      batch.resolve?.();
    } catch (error) {
      batch.reject?.(error);
    }
  }

  sendBatch(frames) {
    const normalized = (Array.isArray(frames) ? frames : []).filter(Boolean);
    if (!normalized.length) return Promise.resolve();
    return invoke("terminal_send_batch", { frames: normalized });
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
      case TERMINAL_FRAME_TYPES.RENDERED_OFFSET:
        return this.queueBatchFrame(frame);
      default:
        this.logger?.warn("Unsupported terminal transport frame", frame);
        return Promise.reject(new Error(`Unsupported terminal transport frame '${frame?.type}'`));
    }
  }
}

export function createTauriTerminalTransport(options = {}) {
  return new TauriTerminalTransport(options);
}
