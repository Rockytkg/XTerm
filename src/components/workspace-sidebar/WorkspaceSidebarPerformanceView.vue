<script setup>
import { useI18n } from "vue-i18n";
import { Activity, Gauge } from "@lucide/vue";
import { formatLatency } from "./performanceFormatters";
import { usePerformanceMetrics } from "./usePerformanceMetrics";
import { connectionCan } from "../../utils/connectionCapabilities";

const props = defineProps({
  activeConnection: { type: Object, default: null },
  runtimeMetrics: { type: Object, default: null },
});

const { t } = useI18n();
const { detailRows, latencyMs, runtimeUnavailable, statCards } = usePerformanceMetrics(props, t);
</script>

<template>
  <div class="workspace-sidebar-pane workspace-sidebar-pane-performance">
    <section class="perf-panel-header">
      <div class="workspace-sidebar-section-head">
        <div class="workspace-sidebar-section-icon">
          <Gauge
            :size="16"
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
        <section class="perf-stat-grid">
          <div
            v-for="card in statCards"
            :key="card.id"
            class="perf-stat-card"
            :class="`perf-tone-${card.tone}`"
          >
            <div class="perf-stat-head">
              <span class="perf-stat-icon">
                <component
                  :is="card.icon"
                  :size="12"
                  stroke-width="1.8"
                />
              </span>
              <span class="perf-stat-label">{{ card.label }}</span>
            </div>
            <div class="perf-stat-value">
              {{ card.value }}
            </div>
            <div
              v-if="card.meter != null"
              class="perf-meter"
            >
              <span :style="{ '--perf-meter-value': `${card.meter}%` }" />
            </div>
            <div class="perf-stat-hint">
              {{ card.hint }}
            </div>
          </div>
        </section>

        <section class="perf-detail-card">
          <div class="perf-detail-title">
            {{ t("overview.runtime.details") }}
          </div>
          <div class="perf-detail-rows">
            <div
              v-for="row in detailRows"
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
