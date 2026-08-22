import { computed } from "vue";
import { Cpu, HardDrive, MemoryStick, Network } from "@lucide/vue";
import { formatBytes } from "../../utils/formatBytes";
import {
  boundedPercent,
  formatDuration,
  formatInteger,
  formatLatency,
  formatPercent,
  formatRate,
} from "./performanceFormatters";

export function usePerformanceMetrics(props, t) {
  const runtimeUnavailable = computed(() => !!props.runtimeMetrics?.unavailable);
  const cpuPercent = computed(() =>
    props.runtimeMetrics?.cpuReady === false
      ? null
      : boundedPercent(props.runtimeMetrics?.cpuPercent),
  );
  const memoryPercent = computed(() => boundedPercent(props.runtimeMetrics?.memoryPercent));
  const diskPercent = computed(() => boundedPercent(props.runtimeMetrics?.diskPercent));
  const swapPercent = computed(() => boundedPercent(props.runtimeMetrics?.swapPercent));

  const memoryInfo = computed(() => {
    const total = props.runtimeMetrics?.memoryTotal;
    const used = props.runtimeMetrics?.memoryUsed;
    const available = props.runtimeMetrics?.memoryAvailable;
    return {
      label: formatPercent(memoryPercent.value),
      used: total ? `${formatBytes(used)} / ${formatBytes(total)}` : "-",
      available: formatBytes(available),
    };
  });

  const diskInfo = computed(() => {
    const total = props.runtimeMetrics?.diskTotal;
    const used = props.runtimeMetrics?.diskUsed;
    const available = props.runtimeMetrics?.diskAvailable;
    return {
      label: formatPercent(diskPercent.value),
      used: total ? `${formatBytes(used)} / ${formatBytes(total)}` : "-",
      available: formatBytes(available),
    };
  });

  const loadAverage = computed(() => {
    const load = props.runtimeMetrics?.loadAverage;
    return load && String(load).trim() ? String(load).trim() : "-";
  });

  const healthItems = computed(() => [
    {
      id: "cpu",
      icon: Cpu,
      label: t("overview.runtime.cpu"),
      value: formatPercent(cpuPercent.value),
      hint: [
        `usr ${formatPercent(props.runtimeMetrics?.cpuUserPercent)}`,
        `sys ${formatPercent(props.runtimeMetrics?.cpuSystemPercent)}`,
      ].join(" / "),
      tone: "accent",
    },
    {
      id: "memory",
      icon: MemoryStick,
      label: t("overview.runtime.memory"),
      value: memoryInfo.value.label,
      hint: memoryInfo.value.used,
      tone: "success",
    },
    {
      id: "disk",
      icon: HardDrive,
      label: t("overview.runtime.disk"),
      value: diskInfo.value.label,
      hint: diskInfo.value.used,
      tone: "warning",
    },
    {
      id: "network",
      icon: Network,
      label: "Network",
      value: formatRate(props.runtimeMetrics?.networkRxRate),
      hint: `TX ${formatRate(props.runtimeMetrics?.networkTxRate)}`,
      tone: "info",
    },
  ]);

  const detailGroups = computed(() => [
    {
      id: "cpu",
      label: t("overview.runtime.cpu"),
      rows: [
        ["User", formatPercent(props.runtimeMetrics?.cpuUserPercent)],
        ["System", formatPercent(props.runtimeMetrics?.cpuSystemPercent)],
        ["I/O wait", formatPercent(props.runtimeMetrics?.cpuIowaitPercent)],
        ["Steal", formatPercent(props.runtimeMetrics?.cpuStealPercent)],
      ],
    },
    {
      id: "memory",
      label: t("overview.runtime.memory"),
      rows: [
        ["Available", memoryInfo.value.available],
        [
          "Swap",
          props.runtimeMetrics?.swapTotal
            ? `${formatBytes(props.runtimeMetrics?.swapUsed)} / ${formatBytes(props.runtimeMetrics?.swapTotal)}`
            : "-",
        ],
        ["Processes", formatInteger(props.runtimeMetrics?.processCount)],
        ["Threads", formatInteger(props.runtimeMetrics?.threadCount)],
      ],
    },
    {
      id: "system",
      label: "System",
      rows: [
        [t("overview.runtime.load"), loadAverage.value],
        [t("overview.session.latency"), formatLatency(props.latencyMs)],
        ["Uptime", formatDuration(props.runtimeMetrics?.uptimeSeconds)],
        ["Inodes", formatPercent(props.runtimeMetrics?.diskInodePercent)],
      ],
    },
  ]);

  return {
    cpuPercent,
    detailGroups,
    diskPercent,
    healthItems,
    memoryInfo,
    memoryPercent,
    runtimeUnavailable,
    swapPercent,
  };
}
