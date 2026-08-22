const UNITS = ["B", "KB", "MB", "GB", "TB"];

/**
 * Formats a byte count into a human-readable string.
 * @param {number} value - The byte value to format
 * @returns {string} Formatted string like "1.5 GB", "0 B", or "-" for invalid input
 */
export function formatBytes(value) {
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes < 0) return "-";
  if (bytes === 0) return "0 B";

  let normalized = bytes;
  let unit = 0;
  while (normalized >= 1024 && unit < UNITS.length - 1) {
    normalized /= 1024;
    unit += 1;
  }
  const precision = normalized >= 10 || unit === 0 ? 0 : 1;
  return `${normalized.toFixed(precision)} ${UNITS[unit]}`;
}

/**
 * Formats a byte rate into a human-readable string.
 * @param {number} value - Bytes per second
 * @returns {string} Formatted string like "1.5 MB/s" or "-" for invalid input
 */
export function formatRate(value) {
  return Number.isFinite(Number(value)) ? `${formatBytes(value)}/s` : "-";
}
