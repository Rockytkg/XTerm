import { computed, reactive, shallowRef, watch } from "vue";
import { boundedPercent } from "../components/workspace-sidebar/performanceFormatters";

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
    lastSampleSignature: "",
  });
}

function pushHistory(history, key, value) {
  const series = history[key];
  series.push(value);
  if (series.length > MAX_POINTS) {
    series.splice(0, series.length - MAX_POINTS);
  }
}

function resetHistory(history, sessionId = "") {
  history.sessionId = sessionId;
  history.cpu = [];
  history.memory = [];
  history.latency = [];
  history.rx = [];
  history.tx = [];
  history.time = [];
  history.version += 1;
  history.lastSampleSignature = "";
}

function sampleSignature(metrics, latencyMs) {
  return [
    metrics.cpuPercent,
    metrics.memoryPercent,
    metrics.diskPercent,
    metrics.networkRxRate,
    metrics.networkTxRate,
    latencyMs,
    metrics.sampleTimestampMs,
  ].join("|");
}

function runtimeSampleSignature(metrics) {
  if (!metrics || metrics.unavailable) return "";
  const latencyMs = Number(metrics.latencyMs);
  return sampleSignature(metrics, Number.isFinite(latencyMs) ? latencyMs : null);
}

export function useWorkspacePerformanceHistory({
  activeConnectionInfo,
  runtimeMetrics,
  openSessions,
}) {
  const histories = shallowRef(new Map());
  const activeConnectionId = computed(() => activeConnectionInfo.value?.id || "");
  const activeSessionId = computed(() => activeConnectionInfo.value?.sessionId || "");

  function setHistory(connectionId, history) {
    histories.value = new Map(histories.value).set(connectionId, history);
  }

  function removeHistoriesExcept(activeIds) {
    let next = histories.value;
    let changed = false;
    for (const connectionId of histories.value.keys()) {
      if (!activeIds.has(connectionId)) {
        if (!changed) next = new Map(histories.value);
        next.delete(connectionId);
        changed = true;
      }
    }
    if (changed) histories.value = next;
  }

  function ensureHistory(
    connectionId = activeConnectionId.value,
    sessionId = activeSessionId.value,
  ) {
    if (!connectionId) return null;
    const existing = histories.value.get(connectionId);
    if (existing) {
      if (sessionId && existing.sessionId && existing.sessionId !== sessionId) {
        resetHistory(existing, sessionId);
      } else if (sessionId && !existing.sessionId) {
        existing.sessionId = sessionId;
      }
      return existing;
    }
    const history = makeHistory(sessionId);
    setHistory(connectionId, history);
    return history;
  }

  function addRuntimeSample(metrics = runtimeMetrics.value) {
    const connectionId = activeConnectionId.value;
    if (!connectionId || !metrics || metrics.unavailable) return;

    const history = ensureHistory(connectionId, activeSessionId.value);
    if (!history) return;

    const latencyMs = Number(metrics.latencyMs);
    const normalizedLatencyMs = Number.isFinite(latencyMs) ? latencyMs : null;
    const signature = sampleSignature(metrics, normalizedLatencyMs);
    if (signature === history.lastSampleSignature) return;

    history.lastSampleSignature = signature;
    pushHistory(
      history,
      "cpu",
      metrics.cpuReady === false ? null : boundedPercent(metrics.cpuPercent),
    );
    pushHistory(history, "memory", boundedPercent(metrics.memoryPercent));
    pushHistory(history, "latency", normalizedLatencyMs);
    pushHistory(
      history,
      "rx",
      Number.isFinite(metrics.networkRxRate) ? Number(metrics.networkRxRate) : null,
    );
    pushHistory(
      history,
      "tx",
      Number.isFinite(metrics.networkTxRate) ? Number(metrics.networkTxRate) : null,
    );
    pushHistory(
      history,
      "time",
      Number.isFinite(metrics.sampleTimestampMs) ? Number(metrics.sampleTimestampMs) : Date.now(),
    );
    history.version += 1;
  }

  watch(
    [activeConnectionId, activeSessionId],
    ([connectionId, sessionId]) => {
      if (connectionId) ensureHistory(connectionId, sessionId);
    },
    { immediate: true },
  );

  watch(
    [activeConnectionId, () => runtimeSampleSignature(runtimeMetrics.value)],
    () => addRuntimeSample(runtimeMetrics.value),
    { immediate: true },
  );

  watch(
    () =>
      (openSessions.value || [])
        .map((s) => s.id)
        .filter(Boolean)
        .join(","),
    (idsStr) => {
      const activeIds = new Set(idsStr ? idsStr.split(",") : []);
      removeHistoriesExcept(activeIds);
    },
  );

  const activePerformanceHistory = computed(() =>
    activeConnectionId.value
      ? histories.value.get(activeConnectionId.value) || EMPTY_HISTORY
      : EMPTY_HISTORY,
  );

  return {
    activePerformanceHistory,
  };
}
