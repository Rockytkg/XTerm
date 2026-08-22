<script setup>
import { useI18n } from "vue-i18n";
import { Activity, Gauge } from "@lucide/vue";
import { formatLatency, formatPercent, formatRate } from "./performanceFormatters";
import { usePerformanceCharts } from "./usePerformanceCharts";
import { usePerformanceMetrics } from "./usePerformanceMetrics";
import { connectionCan } from "../../utils/connectionCapabilities";

const props = defineProps({
  active: { type: Boolean, default: false },
  activeConnection: { type: Object, default: null },
  history: { type: Object, default: null },
  runtimeMetrics: { type: Object, default: null },
  latencyMs: { type: Number, default: null },
});

const { t } = useI18n();
const { cpuPercent, detailGroups, healthItems, memoryInfo, runtimeUnavailable } =
  usePerformanceMetrics(props, t);
const { cpuCanvasRef, memoryCanvasRef, networkCanvasRef } = usePerformanceCharts(props, t);
</script>

<template>
  <div class="workspace-sidebar-pane workspace-sidebar-pane-performance">
    <section class="perf-panel-header">
      <div class="workspace-sidebar-section-head">
        <div class="workspace-sidebar-section-icon">
          <Gauge
            :size="15"
            stroke-width="1.8"
          />
        </div>
        <div class="perf-panel-heading">
          <div class="workspace-sidebar-section-kicker">
            {{ t("sidebar.performanceKicker") }}
          </div>
          <div class="workspace-sidebar-section-title">
            {{ t("overview.runtime.title") }}
          </div>
        </div>
      </div>
      <div
        v-if="connectionCan(activeConnection, 'metrics') && !runtimeUnavailable"
        class="perf-live-pill"
      >
        <span class="perf-live-dot" />
        <span>{{ formatLatency(latencyMs) }}</span>
      </div>
    </section>

    <template v-if="connectionCan(activeConnection, 'metrics')">
      <div
        v-if="!runtimeUnavailable"
        class="perf-dashboard"
      >
        <section class="perf-health-grid">
          <div
            v-for="item in healthItems"
            :key="item.id"
            class="perf-health-card"
            :class="`perf-tone-${item.tone}`"
          >
            <div class="perf-health-top">
              <component
                :is="item.icon"
                :size="13"
                stroke-width="1.8"
              />
              <span>{{ item.label }}</span>
            </div>
            <strong>{{ item.value }}</strong>
            <small>{{ item.hint }}</small>
          </div>
        </section>

        <section class="perf-trend-grid">
          <div class="perf-trend-card">
            <div class="perf-trend-head">
              <span>{{ t("overview.runtime.cpu") }}</span>
              <strong>{{ formatPercent(cpuPercent) }}</strong>
            </div>
            <div class="perf-chart-shell">
              <canvas ref="cpuCanvasRef" />
            </div>
          </div>
          <div class="perf-trend-card">
            <div class="perf-trend-head">
              <span>{{ t("overview.runtime.memory") }}</span>
              <strong>{{ memoryInfo.label }}</strong>
            </div>
            <div class="perf-chart-shell">
              <canvas ref="memoryCanvasRef" />
            </div>
          </div>
          <div class="perf-trend-card perf-trend-card-wide">
            <div class="perf-trend-head">
              <span>Network</span>
              <strong>RX {{ formatRate(runtimeMetrics?.networkRxRate) }}</strong>
            </div>
            <div class="perf-chart-shell">
              <canvas ref="networkCanvasRef" />
            </div>
          </div>
        </section>

        <section class="perf-detail-grid">
          <div
            v-for="group in detailGroups"
            :key="group.id"
            class="perf-detail-group"
          >
            <div class="perf-detail-title">
              {{ group.label }}
            </div>
            <div
              v-for="row in group.rows"
              :key="row[0]"
              class="perf-detail-row"
            >
              <span>{{ row[0] }}</span>
              <strong>{{ row[1] }}</strong>
            </div>
          </div>
        </section>
      </div>

      <div
        v-else
        class="perf-empty"
      >
        <div class="workspace-sidebar-inline-note">
          {{ t("overview.runtime.unavailable") }}
        </div>
      </div>
    </template>

    <section
      v-else
      class="perf-empty"
    >
      <div class="workspace-sidebar-empty-state">
        <Activity
          :size="18"
          stroke-width="1.8"
          class="text-text-tertiary"
        />
        <div>
          <strong>{{ t("sidebar.performanceUnavailableTitle") }}</strong>
          <span>{{ t("sidebar.performanceUnavailableDesc") }}</span>
        </div>
      </div>
    </section>
  </div>
</template>
