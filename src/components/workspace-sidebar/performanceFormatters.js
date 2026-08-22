import { formatRate as formatByteRate } from "../../utils/formatBytes";

const sampleTimeFormatter = new Intl.DateTimeFormat(undefined, {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

export function boundedPercent(value) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.min(100, Math.max(0, number)) : null;
}

export function formatLatency(value) {
  return Number.isFinite(Number(value)) ? `${Number(value).toFixed(0)} ms` : "-";
}

export function formatPercent(value) {
  return Number.isFinite(Number(value)) ? `${Number(value).toFixed(1)}%` : "-";
}

export function formatInteger(value) {
  return Number.isFinite(Number(value)) ? Number(value).toLocaleString() : "-";
}

export function formatDuration(value) {
  const seconds = Number(value);
  if (!Number.isFinite(seconds) || seconds < 0) return "-";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  if (days > 0) return `${days}d ${hours}h`;
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${minutes}m`;
}

export function formatSampleTime(timestampMs) {
  const timestamp = Number(timestampMs);
  if (!Number.isFinite(timestamp) || timestamp <= 0) return "-";
  return sampleTimeFormatter.format(new Date(timestamp));
}

export function formatRate(value) {
  return formatByteRate(value);
}
