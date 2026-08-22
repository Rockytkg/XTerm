<script setup>
import { useI18n } from "vue-i18n";
import "../../styles/views-dashboard.scss";

const { t } = useI18n();
</script>

<template>
  <div class="dashboard-empty">
    <div class="dashboard-empty-icon">
      <svg
        width="40"
        height="40"
        viewBox="0 0 40 40"
        fill="none"
      >
        <rect
          width="40"
          height="40"
          rx="12"
          fill="var(--accent-light)"
        />
        <path
          d="M10 14l8 6-8 6M20 26h10"
          stroke="var(--accent)"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </div>
    <div class="dashboard-empty-title">
      {{ t("dashboard.emptyTitle") }}
    </div>
    <div class="dashboard-empty-desc">
      {{ t("dashboard.emptyDesc") }}
    </div>
  </div>
</template>
