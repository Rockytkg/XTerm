/**
 * renderedOffset 上报节流器。
 *
 * 触发点只有“背压低水位恢复”：高吞吐期间恢复事件密集，节流器保证
 * 每 minIntervalMs 或每 minBytesBetweenReports 至少上报一次最近消费的
 * endOffset；低吞吐（长时间不触发恢复）或完全没有消费数据时不上报。
 */

const RENDERED_OFFSET_REPORT_MIN_INTERVAL_MS = 500;
const RENDERED_OFFSET_REPORT_MIN_BYTES = 4 * 1024 * 1024;

export function createRenderedOffsetReporter({
  send,
  minIntervalMs = RENDERED_OFFSET_REPORT_MIN_INTERVAL_MS,
  minBytesBetweenReports = RENDERED_OFFSET_REPORT_MIN_BYTES,
  now = () => Date.now(),
}) {
  let lastReportAt = 0;
  let bytesSinceReport = 0;
  let latestOffset = null;

  function noteConsumed(byteLength, endOffset) {
    if (!Number.isSafeInteger(endOffset)) return;
    latestOffset = endOffset;
    bytesSinceReport += Math.max(0, Number(byteLength) || 0);
  }

  function onBackpressureResume() {
    if (latestOffset === null) return;
    const current = now();
    const intervalElapsed = lastReportAt === 0 || current - lastReportAt >= minIntervalMs;
    if (!intervalElapsed && bytesSinceReport < minBytesBetweenReports) return;
    const offset = latestOffset;
    lastReportAt = current;
    bytesSinceReport = 0;
    send(offset);
  }

  function reset() {
    lastReportAt = 0;
    bytesSinceReport = 0;
    latestOffset = null;
  }

  return {
    noteConsumed,
    onBackpressureResume,
    reset,
  };
}
