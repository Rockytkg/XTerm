import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import {
  CategoryScale,
  Chart,
  Filler,
  LinearScale,
  LineController,
  LineElement,
  PointElement,
  Tooltip,
} from "chart.js";
import { formatPercent, formatRate, formatSampleTime } from "./performanceFormatters";
import { createRafThrottle } from "../../utils/schedulers";

Chart.register(
  LineController,
  LineElement,
  PointElement,
  LinearScale,
  CategoryScale,
  Tooltip,
  Filler,
);

function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function resolveColor(varName, fallback) {
  const raw = cssVar(varName);
  if (!raw) return fallback;
  const el = document.createElement("div");
  el.style.color = raw;
  el.style.display = "none";
  document.body.appendChild(el);
  const resolved = getComputedStyle(el).color;
  document.body.removeChild(el);
  return resolved || fallback;
}

function withAlpha(color, alpha) {
  if (color.startsWith("rgba(")) {
    return color.replace(/rgba\(([^)]+),\s*[\d.]+\)/, `rgba($1, ${alpha})`);
  }
  if (color.startsWith("rgb(")) {
    return color.replace(/^rgb\(/, "rgba(").replace(/\)$/, `, ${alpha})`);
  }
  return color;
}

function makeLineFill(color) {
  return (context) => {
    const { chart } = context;
    const { chartArea, ctx } = chart;
    if (!chartArea) return withAlpha(color, 0.14);
    const gradient = ctx.createLinearGradient(0, chartArea.top, 0, chartArea.bottom);
    gradient.addColorStop(0, withAlpha(color, 0.22));
    gradient.addColorStop(1, withAlpha(color, 0.02));
    return gradient;
  };
}

function makeDataset({ label, varName, fallback, fill = true }) {
  const color = resolveColor(varName, fallback);
  return {
    label,
    data: [],
    borderColor: color,
    backgroundColor: fill ? makeLineFill(color) : withAlpha(color, 0.08),
    borderWidth: 2,
    borderCapStyle: "round",
    pointRadius: 0,
    pointHoverRadius: 3,
    fill,
    spanGaps: true,
    tension: 0.38,
  };
}

function makeLineChart(
  canvas,
  datasets,
  { maxY = 100, fixedMax = true, valueFormatter = formatPercent } = {},
) {
  if (!canvas) return null;
  return new Chart(canvas, {
    type: "line",
    data: { labels: [], datasets },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false,
      interaction: { intersect: false, mode: "index" },
      layout: { padding: { top: 2, right: 2, bottom: 0, left: 2 } },
      scales: {
        x: {
          display: true,
          grid: { display: false },
          border: { display: false },
          ticks: {
            autoSkip: true,
            maxTicksLimit: 3,
            color: resolveColor("--text-tertiary", "rgb(120,120,120)"),
            maxRotation: 0,
            minRotation: 0,
            font: { size: 9 },
          },
        },
        y: { display: false, min: 0, ...(fixedMax ? { max: maxY } : { suggestedMax: maxY }) },
      },
      plugins: {
        legend: { display: false },
        tooltip: {
          displayColors: false,
          callbacks: {
            title: (items) => items?.[0]?.label || "",
            label: (ctx) => {
              const value = Number.isFinite(ctx.parsed.y) ? valueFormatter(ctx.parsed.y) : "-";
              return `${ctx.dataset.label}: ${value}`;
            },
          },
        },
      },
    },
  });
}

function replaceArray(target, values, mapper = (value) => value) {
  target.length = values.length;
  for (let index = 0; index < values.length; index += 1) {
    target[index] = mapper(values[index]);
  }
}

function toChartValue(value) {
  return Number.isFinite(value) ? value : null;
}

export function usePerformanceCharts(props, t) {
  const cpuCanvasRef = ref(null);
  const memoryCanvasRef = ref(null);
  const networkCanvasRef = ref(null);
  let charts = {};
  const scheduleChartsFromHistoryUpdate = createRafThrottle(updateChartsFromHistory);

  function historySeries(key) {
    const values = props.history?.[key];
    return Array.isArray(values) ? values : [];
  }

  function updateSingleChart(chart, historyKey, labels) {
    if (!chart) return;
    const pts = historySeries(historyKey);
    chart.data.labels = labels;
    replaceArray(chart.data.datasets[0].data, pts, toChartValue);
    chart.update("none");
  }

  function updateNetworkChart(labels) {
    if (!charts.network) return;
    const rx = historySeries("rx");
    charts.network.data.labels = labels;
    replaceArray(charts.network.data.datasets[0].data, rx, toChartValue);
    replaceArray(charts.network.data.datasets[1].data, historySeries("tx"), toChartValue);
    charts.network.update("none");
  }

  function updateChartsFromHistory() {
    const labels = historySeries("time").map(formatSampleTime);
    updateSingleChart(charts.cpu, "cpu", labels);
    updateSingleChart(charts.memory, "memory", labels);
    updateNetworkChart(labels);
  }

  function destroyCharts() {
    charts.cpu?.destroy();
    charts.memory?.destroy();
    charts.network?.destroy();
    charts = {};
  }

  function initCharts() {
    destroyCharts();
    charts.cpu = makeLineChart(cpuCanvasRef.value, [
      makeDataset({ label: "CPU", varName: "--accent", fallback: "rgb(59,130,246)" }),
    ]);
    charts.memory = makeLineChart(memoryCanvasRef.value, [
      makeDataset({
        label: t("overview.runtime.memory"),
        varName: "--success",
        fallback: "rgb(34,197,94)",
      }),
    ]);
    charts.network = makeLineChart(
      networkCanvasRef.value,
      [
        makeDataset({ label: "RX", varName: "--info", fallback: "rgb(6,182,212)" }),
        makeDataset({
          label: "TX",
          varName: "--warning",
          fallback: "rgb(245,158,11)",
          fill: false,
        }),
      ],
      { maxY: 1024, fixedMax: false, valueFormatter: formatRate },
    );
    updateChartsFromHistory();
  }

  function refreshVisibleCharts() {
    if (!props.active) return;
    nextTick(() => {
      if (!Object.keys(charts).length) {
        initCharts();
        return;
      }
      charts.cpu?.resize();
      charts.memory?.resize();
      charts.network?.resize();
      updateChartsFromHistory();
    });
  }

  watch(
    () => props.active,
    () => refreshVisibleCharts(),
    { flush: "post" },
  );

  watch(
    () => props.history?.version,
    () => {
      if (props.active) scheduleChartsFromHistoryUpdate();
    },
    { flush: "post" },
  );

  watch(
    [cpuCanvasRef, memoryCanvasRef, networkCanvasRef],
    (refs) => {
      if (!refs.every(Boolean)) {
        destroyCharts();
        return;
      }
      if (props.active) nextTick(initCharts);
    },
    { flush: "post" },
  );

  onBeforeUnmount(() => {
    scheduleChartsFromHistoryUpdate.cancel();
    destroyCharts();
  });

  return {
    cpuCanvasRef,
    memoryCanvasRef,
    networkCanvasRef,
  };
}
