import {
  createAttachFrame,
  createDetachFrame,
  createInputBytesFrame,
  createInputTextFrame,
  TERMINAL_FRAME_TYPES,
} from "./protocol/frames.js";
import { connectionCan } from "../connectionCapabilities.js";
import { createEventBus } from "../createEventBus.js";
import { leaseOwnsPayload } from "./sessionIdentity.js";

export const TERMINAL_SESSION_RUNTIME_EVENTS = Object.freeze({
  SESSION_DATA: "session:data",
  CHANNEL_CHANGED: "channel:changed",
});

const CONNECTION_PHASE = Object.freeze({
  HOST_KEY_CHALLENGE: "hostKeyChallenge",
  SERIAL_BAUD_DETECTION: "serialBaudDetection",
});

const TERMINAL_STATUS = Object.freeze({
  CLOSED: "closed",
  CONNECTED: "connected",
  CONNECTING: "connecting",
  FAILED: "failed",
  HOST_KEY_CONFIRMATION: "hostKeyConfirmation",
  SECURE_SESSION: "secureSession",
});

const MAX_PENDING_INPUTS = 256;

class TerminalSessionRuntimeController {
  constructor({
    drainOutput,
    dropOutput,
    getContext,
    logger,
    queueResizeSync,
    releaseStatus,
    setActiveSessionChannel,
    transport,
    writeStatus,
  }) {
    this.drainOutput = drainOutput;
    this.dropOutput = dropOutput;
    this.getContext = getContext;
    this.logger = logger;
    this.queueResizeSync = queueResizeSync;
    this.releaseStatus = releaseStatus;
    this.setActiveSessionChannel = setActiveSessionChannel;
    this.transport = transport;
    this.writeStatus = writeStatus;

    this.bus = createEventBus({ logger });
    this.channel = null;
    this.closingChannel = { sessionId: "", channelId: null };
    this.activationKey = 0;
    this.transitionChain = Promise.resolve();
    this.pendingConnectionStatuses = [];
    this.pendingInputs = [];
    this.inputBarrierSessionId = "";
    this.nextInputSequence = 1;
    this.suspended = false;
  }

  on(type, handler) {
    return this.bus.on(type, handler);
  }

  emit(type, payload) {
    return this.bus.emit(type, payload);
  }

  currentContext() {
    return this.getContext?.() ?? {};
  }

  currentChannel() {
    return this.channel;
  }

  currentChannelId() {
    return this.channel?.channelId ?? null;
  }

  hasActiveChannel() {
    return !!this.channel;
  }

  canSyncBackend() {
    const context = this.currentContext();
    return !!this.channel && !!context.isForeground && connectionCan(context, "resize");
  }

  outputPayloadState(extra = {}) {
    const context = this.currentContext();
    return {
      sessionId: context.sessionId || "",
      sessionChannelId: this.currentChannelId(),
      closingSessionId: this.closingChannel.sessionId,
      closingSessionChannelId: this.closingChannel.channelId,
      ...extra,
    };
  }

  resetOutputRouting() {
    this.closingChannel = { sessionId: "", channelId: null };
  }

  acceptsOutputPayload(payload, sessionId, channelId = null) {
    if (!payload || payload.sessionId !== sessionId) return false;
    if (channelId === null || channelId === undefined) return true;
    return Number(payload.channelId) === Number(channelId);
  }

  acceptsLeasePayload(payload, lease) {
    return leaseOwnsPayload(lease, payload);
  }

  activeInputTarget(data) {
    const context = this.currentContext();
    const channel = this.channel;
    if (
      !channel ||
      this.inputBarrierSessionId === context.sessionId ||
      context.connectionPhase === CONNECTION_PHASE.SERIAL_BAUD_DETECTION ||
      !context.isForeground ||
      context.connectionStatus !== "connected" ||
      !context.sessionId ||
      channel.sessionId !== context.sessionId ||
      !data
    ) {
      return null;
    }
    return {
      sessionId: channel.sessionId,
      channelId: channel.channelId,
    };
  }

  canBufferInput(data) {
    const context = this.currentContext();
    return (
      !!data &&
      !!context.isForeground &&
      context.connectionPhase !== CONNECTION_PHASE.SERIAL_BAUD_DETECTION &&
      context.connectionStatus === "connected" &&
      !!context.sessionId &&
      !context.disposed
    );
  }

  sendInput(target, data, createFrame, errorLabel) {
    const inputSequence = this.nextInputSequence++;
    this.transport.send(createFrame({ ...target, inputSequence, data })).catch((error) => {
      this.logger?.error(errorLabel, error);
    });
  }

  queueInput(data, createFrame, errorLabel) {
    const target = this.activeInputTarget(data);
    if (target) {
      this.sendInput(target, data, createFrame, errorLabel);
      return;
    }
    if (!this.canBufferInput(data)) return;
    // 后端 attach 可能永不返回，pendingInputs 需有上限；
    // 超限时丢弃最旧的输入，保留较新的按键更贴近用户当前意图
    if (this.pendingInputs.length >= MAX_PENDING_INPUTS) {
      this.pendingInputs.shift();
    }
    this.pendingInputs.push({
      createFrame,
      data,
      errorLabel,
      sessionId: this.currentContext().sessionId,
    });
    void this.activate();
  }

  queueText(data) {
    this.queueInput(data, createInputTextFrame, "Failed to write backend terminal:");
  }

  queueBytes(dataBase64) {
    this.queueInput(
      dataBase64,
      ({ sessionId, channelId, data }) =>
        createInputBytesFrame({ sessionId, channelId, dataBase64: data }),
      "Failed to write backend terminal bytes:",
    );
  }

  syncRuntimeResources() {
    const context = this.currentContext();
    if (context.connectionId && context.sessionId) {
      void this.activate();
      return;
    }
    void this.deactivate();
  }

  handleConnectionStatus(status, phase) {
    const context = this.currentContext();
    if (!context.hasActiveConnection) {
      this.pendingConnectionStatuses = [];
      return;
    }

    if (!context.terminalReady) {
      this.rememberPendingConnectionStatus(context.connectionId, status, phase);
      return;
    }

    if (this.pendingConnectionStatuses.length) {
      const pending = this.pendingConnectionStatuses.filter(
        (entry) => entry.connectionId === context.connectionId,
      );
      this.pendingConnectionStatuses = [];
      for (const entry of pending) {
        this.applyConnectionStatus(entry.status, entry.phase);
      }
      const last = pending.at(-1);
      if (last?.status === status && last?.phase === phase) return;
    }

    this.applyConnectionStatus(status, phase);
  }

  rememberPendingConnectionStatus(connectionId, status, phase) {
    if (!connectionId || !status) return;
    const queue = this.pendingConnectionStatuses.filter(
      (entry) => entry.connectionId === connectionId,
    );
    const last = queue.at(-1);
    if (last?.status === status && last?.phase === phase) return;
    this.pendingConnectionStatuses = [
      ...queue,
      { connectionId, status, phase: phase || null },
    ].slice(-4);
  }

  applyConnectionStatus(status, phase) {
    const context = this.currentContext();
    if (phase === CONNECTION_PHASE.HOST_KEY_CHALLENGE) {
      this.writeStatus?.(TERMINAL_STATUS.HOST_KEY_CONFIRMATION);
    } else if (status === "connecting") {
      this.writeStatus?.(TERMINAL_STATUS.CONNECTING);
    } else if (status === "authenticating") {
      this.writeStatus?.(TERMINAL_STATUS.SECURE_SESSION);
    } else if (status === "connected") {
      this.writeStatus?.(TERMINAL_STATUS.CONNECTED);
      if (context.sessionId) void this.activate();
    } else if (status === "failed") {
      this.clearChannelLocally({ recordClosing: true });
      this.writeStatus?.(TERMINAL_STATUS.FAILED);
    } else if (status === "closed") {
      this.clearChannelLocally({ recordClosing: true });
      this.writeStatus?.(TERMINAL_STATUS.CLOSED);
    } else if (status === "disconnecting" || status === "idle") {
      this.releaseStatus?.();
    }
  }

  flushPendingInputs() {
    if (!this.pendingInputs.length) return;
    const context = this.currentContext();
    const channel = this.channel;
    if (
      !channel ||
      !context.isForeground ||
      context.connectionStatus !== "connected" ||
      channel.sessionId !== context.sessionId
    ) {
      this.pendingInputs = this.pendingInputs.filter(
        (entry) => entry.sessionId === context.sessionId,
      );
      return;
    }
    const pending = this.pendingInputs;
    this.pendingInputs = [];
    const target = {
      sessionId: channel.sessionId,
      channelId: channel.channelId,
    };
    for (const entry of pending) {
      if (entry.sessionId !== channel.sessionId) continue;
      this.sendInput(target, entry.data, entry.createFrame, entry.errorLabel);
    }
  }

  async activate() {
    return this.enqueueTransition(() => this.activateNow());
  }

  enqueueTransition(action) {
    const transition = this.transitionChain.then(action, action);
    this.transitionChain = transition.catch(() => {});
    return transition;
  }

  async activateNow() {
    const context = this.currentContext();
    const sessionId = context.sessionId || "";
    if (!this.canActivate(context)) return false;
    if (this.channel?.sessionId === sessionId) return true;
    if (this.channel && !(await this.deactivateNow())) return false;
    if (!this.canActivate() || this.currentContext().sessionId !== sessionId) return false;
    return this.runActivation(sessionId);
  }

  async deactivate() {
    return this.enqueueTransition(() => this.deactivateNow());
  }

  async deactivateNow() {
    const context = this.currentContext();
    const channel = this.channel;
    const sessionId = channel?.sessionId || context.sessionId || "";
    const channelId = channel?.channelId ?? null;

    if (!sessionId || !channelId) {
      this.clearChannelLocally({ recordClosing: false, sessionId, channelId });
      return true;
    }

    this.inputBarrierSessionId = sessionId;
    try {
      await this.transport.send(createDetachFrame(sessionId, channelId));
      await this.drainOutput?.();
      this.clearChannelLocally({ recordClosing: false, sessionId, channelId });
      this.dropOutput?.();
      return true;
    } catch (error) {
      this.logger?.error("Failed to deactivate backend session:", error);
      await this.drainOutput?.();
      this.clearChannelLocally({ recordClosing: false, sessionId, channelId });
      this.dropOutput?.();
      return false;
    }
  }

  canActivate(context = this.currentContext()) {
    return (
      !this.suspended &&
      !!context.terminalReady &&
      !!context.sessionId &&
      context.connectionStatus === "connected" &&
      !context.disposed
    );
  }

  isSuspended() {
    return this.suspended;
  }

  /**
   * 后台挂起：detach channel，后端会话与 replay cache 继续运行。
   * 挂起期间输入被既有 isForeground 门禁（activeInputTarget/canBufferInput）
   * 阻断——挂起必然处于后台，输入既不发送也不缓冲，也不会唤醒 attach；
   * 回前台时由 resumeFromBackground 统一恢复。先置标志位再入队，保证
   * 链上排在前面的 activate 完成之后不会留下已 attach 的“挂起”会话。
   */
  suspendForBackground() {
    this.suspended = true;
    return this.enqueueTransition(() => this.deactivateNow());
  }

  /**
   * 回前台恢复：重新 attach。前端 offset 游标未失效时，后端从
   * delivered_offset 增量补发 + classify 游标去重，xterm 不需要 reset；
   * 游标失效（会话重建）由 sessionId watch 走既有的 reset + 全量回放路径。
   */
  resumeFromBackground() {
    this.suspended = false;
    return this.enqueueTransition(() => this.activateNow());
  }

  async runActivation(sessionId) {
    const activationKey = ++this.activationKey;
    const pendingOutputPayloads = [];
    let outputLease = null;
    let channelReady = false;
    try {
      const lease = await this.transport.send(
        createAttachFrame(sessionId, (frame) => {
          if (frame?.type === TERMINAL_FRAME_TYPES.OUTPUT) {
            if (!this.acceptsOutputPayload(frame.payload, sessionId)) return;
            if (channelReady) {
              if (outputLease && this.acceptsLeasePayload(frame.payload, outputLease)) {
                this.emit(TERMINAL_SESSION_RUNTIME_EVENTS.SESSION_DATA, frame.payload);
              }
            } else {
              pendingOutputPayloads.push(frame.payload);
            }
          }
        }),
      );
      const nextChannelId = lease?.channelId;
      const alreadyActive = lease?.alreadyActive === true;
      if (!this.isActivationFresh(activationKey, sessionId) || !Number.isFinite(nextChannelId)) {
        const currentChannelOwnsResponse =
          this.channel?.sessionId === sessionId && this.channel?.channelId === nextChannelId;
        if (Number.isFinite(nextChannelId) && !alreadyActive && !currentChannelOwnsResponse) {
          await this.transport.send(createDetachFrame(sessionId, nextChannelId)).catch(() => {});
        }
        // 无论是否发了 detach，都本地丢弃本次 attach 注册的订阅；
        // 否则其闭包 channelReady 恒为 false，pendingOutputPayloads 会无限增长。
        this.transport.dropSubscription?.(
          sessionId,
          Number.isFinite(nextChannelId) ? nextChannelId : null,
        );
        return false;
      }

      const context = this.currentContext();
      outputLease = lease;
      this.channel = {
        ...lease,
        connectionId: context.connectionId || "",
      };
      this.nextInputSequence = 1;
      this.setActiveSessionChannel?.(this.channel.connectionId, nextChannelId);
      this.closingChannel = { sessionId: "", channelId: null };
      this.emit(TERMINAL_SESSION_RUNTIME_EVENTS.CHANNEL_CHANGED, this.channel);
      this.inputBarrierSessionId = sessionId;
      channelReady = true;
      for (const payload of pendingOutputPayloads) {
        if (!this.acceptsLeasePayload(payload, outputLease)) continue;
        this.emit(TERMINAL_SESSION_RUNTIME_EVENTS.SESSION_DATA, payload);
      }
      await this.drainOutput?.();
      if (!this.isActivationFresh(activationKey, sessionId)) {
        return false;
      }
      this.inputBarrierSessionId = "";
      this.flushPendingInputs();
      this.queueResizeSync?.();
      return true;
    } catch (error) {
      this.logger?.error("Failed to activate backend session:", error);
      return false;
    } finally {
      if (this.inputBarrierSessionId === sessionId) {
        this.inputBarrierSessionId = "";
      }
    }
  }

  isActivationFresh(activationKey, sessionId) {
    const context = this.currentContext();
    return (
      !context.disposed &&
      activationKey === this.activationKey &&
      context.connectionStatus === "connected" &&
      context.sessionId === sessionId
    );
  }

  clearChannelLocally({ recordClosing = false, sessionId = "", channelId = null } = {}) {
    const context = this.currentContext();
    const current = this.channel;
    const resolvedSessionId = sessionId || current?.sessionId || context.sessionId || "";
    const resolvedChannelId = channelId ?? current?.channelId ?? null;
    const connectionId = current?.connectionId || context.connectionId || "";

    if (recordClosing) {
      this.closingChannel = {
        sessionId: resolvedSessionId,
        channelId: resolvedChannelId,
      };
    }

    this.channel = null;
    this.nextInputSequence = 1;
    this.setActiveSessionChannel?.(connectionId, null);
    // 本地清理 channel 时同步丢弃 transport 侧残留订阅（failed/closed
    // 路径不会发 detach，否则会泄漏到组件卸载才回收）；
    // deactivate 路径已发过 detach，这里是无害的空操作。
    this.transport.dropSubscription?.(resolvedSessionId, resolvedChannelId);
    this.activationKey += 1;
    this.inputBarrierSessionId = "";
    this.pendingInputs = this.pendingInputs.filter(
      (entry) => entry.sessionId && entry.sessionId !== resolvedSessionId,
    );
    this.emit(TERMINAL_SESSION_RUNTIME_EVENTS.CHANNEL_CHANGED, null);
  }
}

export function createTerminalSessionRuntimeController(options) {
  return new TerminalSessionRuntimeController(options);
}
