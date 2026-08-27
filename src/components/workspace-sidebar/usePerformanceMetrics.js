import { computed } from "vue";
import { Cpu, HardDrive, MemoryStick, Network } from "@lucide/vue";
import { formatBytes, formatRate } from "../../utils/formatBytes";
import {
  boundedPercent,
  formatDuration,
  formatInteger,
  formatPercent,
} from "./performanceFormatters";

function usageText(used, total) {
  return total ? `${formatBytes(used)} / ${formatBytes(total)}` : "-";
}

export function usePerformanceMetrics(props, t) {
  const runtimeUnavailable = computed(() => !!props.runtimeMetrics?.unavailable);

  const latencyMs = computed(() => {
    const value = Number(props.runtimeMetrics?.latencyMs);
    return Number.isFinite(value) ? value : null;
  });

  const cpuPercent = computed(() =>
    props.runtimeMetrics?.cpuReady === false
      ? null
      : boundedPercent(props.runtimeMetrics?.cpuPercent),
  );
  const memoryPercent = computed(() => boundedPercent(props.runtimeMetrics?.memoryPercent));
  const diskPercent = computed(() => boundedPercent(props.runtimeMetrics?.diskPercent));

  const statCards = computed(() => {
    const metrics = props.runtimeMetrics || {};
    return [
      {
        id: "cpu",
        icon: Cpu,
        tone: "accent",
        label: t("overview.runtime.cpu"),
        value: formatPercent(cpuPercent.value),
        meter: cpuPercent.value,
        hint: `usr ${formatPercent(metrics.cpuUserPercent)} / sys ${formatPercent(metrics.cpuSystemPercent)}`,
      },
      {
        id: "memory",
        icon: MemoryStick,
        tone: "success",
        label: t("overview.runtime.memory"),
        value: formatPercent(memoryPercent.value),
        meter: memoryPercent.value,
        hint: usageText(metrics.memoryUsed, metrics.memoryTotal),
      },
      {
        id: "disk",
        icon: HardDrive,
        tone: "warning",
        label: t("overview.runtime.disk"),
        value: formatPercent(diskPercent.value),
        meter: diskPercent.value,
        hint: usageText(metrics.diskUsed, metrics.diskTotal),
      },
      {
        id: "network",
        icon: Network,
        tone: "info",
        label: t("overview.runtime.network"),
        value: `↓ ${formatRate(metrics.networkRxRate)}`,
        meter: null,
        hint: `↑ ${formatRate(metrics.networkTxRate)}`,
      },
    ];
  });

  const detailRows = computed(() => {
    const metrics = props.runtimeMetrics || {};
    const load = String(metrics.loadAverage || "").trim();
    return [
      [t("overview.runtime.load"), load || "-"],
      [t("overview.runtime.uptime"), formatDuration(metrics.uptimeSeconds)],
      [t("overview.runtime.processes"), formatInteger(metrics.processCount)],
      [t("overview.runtime.threads"), formatInteger(metrics.threadCount)],
      [t("overview.runtime.available"), formatBytes(metrics.memoryAvailable)],
      [
        t("overview.runtime.swap"),
        metrics.swapTotal ? usageText(metrics.swapUsed, metrics.swapTotal) : "-",
      ],
      [t("overview.runtime.iowait"), formatPercent(metrics.cpuIowaitPercent)],
      [t("overview.runtime.steal"), formatPercent(metrics.cpuStealPercent)],
      [t("overview.runtime.inodes"), formatPercent(metrics.diskInodePercent)],
    ];
  });

  return {
    cpuPercent,
    detailRows,
    latencyMs,
    memoryPercent,
    runtimeUnavailable,
    statCards,
  };
}
