import { computed, reactive, shallowRef, watch } from "vue";
import { boundedPercent } from "../components/workspace-sidebar/performanceFormatters";

// 采样间隔由后端监控循环决定（约 2s），42 个点大约覆盖最近 80 秒。
const MAX_POINTS = 42;
const EMPTY_HISTORY = Object.freeze({
  cpu: Object.freeze([]),
  memory: Object.freeze([]),
  latency: Object.freeze([]),
  rx: Object.freeze([]),
  tx: Object.freeze([]),
  time: Object.freeze([]),
  version: 0,
});

function makeHistory(sessionId = "") {
  return reactive({
    sessionId,
    cpu: [],
    memory: [],
    latency: [],
    rx: [],
    tx: [],
    time: [],
    version: 0,
    lastSampleAt: 0,
  });
}

function pushPoint(series, value) {
  series.push(value);
  if (series.length > MAX_POINTS) {
    series.splice(0, series.length - MAX_POINTS);
  }
}

function pushSample(history, metrics) {
  const sampleAt = Number.isFinite(metrics.sampleTimestampMs)
    ? Number(metrics.sampleTimestampMs)
    : Date.now();
  // 切回标签页时会带着该连接的存量采样触发本逻辑；时间戳相同说明已入列，直接跳过。
  if (sampleAt === history.lastSampleAt) return;
  history.lastSampleAt = sampleAt;

  const latencyMs = Number(metrics.latencyMs);
  pushPoint(history.cpu, metrics.cpuReady === false ? null : boundedPercent(metrics.cpuPercent));
  pushPoint(history.memory, boundedPercent(metrics.memoryPercent));
  pushPoint(history.latency, Number.isFinite(latencyMs) ? latencyMs : null);
  pushPoint(
    history.rx,
    Number.isFinite(metrics.networkRxRate) ? Number(metrics.networkRxRate) : null,
  );
  pushPoint(
    history.tx,
    Number.isFinite(metrics.networkTxRate) ? Number(metrics.networkTxRate) : null,
  );
  pushPoint(history.time, sampleAt);
  history.version += 1;
}

export function useWorkspacePerformanceHistory({
  activeConnectionInfo,
  runtimeMetrics,
  openSessions,
}) {
  // key 为 connectionId。Map 本身无需响应式：面板只读当前历史对象，
  // 其 version 字段是响应式的，图表据此刷新。
  const histories = new Map();
  const activeConnectionId = computed(() => activeConnectionInfo.value?.id || "");
  const activeSessionId = computed(() => activeConnectionInfo.value?.sessionId || "");
  const activePerformanceHistory = shallowRef(EMPTY_HISTORY);

  // sessionRegistry.setRuntimeMetrics 会按值去重，仅在新采样到达时才替换对象引用，
  // 因此这里直接监听引用变化即可，无需额外的签名比对。
  watch(
    () => [activeConnectionId.value, activeSessionId.value, runtimeMetrics.value],
    () => {
      const connectionId = activeConnectionId.value;
      if (!connectionId) {
        activePerformanceHistory.value = EMPTY_HISTORY;
        return;
      }

      const sessionId = activeSessionId.value;
      let history = histories.get(connectionId);
      if (history && sessionId && history.sessionId && history.sessionId !== sessionId) {
        // 同一连接换了后端会话（重连等）：旧曲线对新会话没有意义，重新建档。
        history = undefined;
      }
      if (!history) {
        history = makeHistory(sessionId);
        histories.set(connectionId, history);
      } else if (sessionId && !history.sessionId) {
        history.sessionId = sessionId;
      }
      activePerformanceHistory.value = history;

      const metrics = runtimeMetrics.value;
      if (metrics && !metrics.unavailable) {
        pushSample(history, metrics);
      }
    },
    { immediate: true },
  );

  watch(
    () =>
      (openSessions.value || [])
        .map((session) => session.id)
        .filter(Boolean)
        .join(","),
    (ids) => {
      const activeIds = new Set(ids ? ids.split(",") : []);
      for (const connectionId of histories.keys()) {
        if (!activeIds.has(connectionId)) {
          histories.delete(connectionId);
        }
      }
    },
  );

  return {
    activePerformanceHistory,
  };
}
