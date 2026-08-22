// 错误解析已迁移到 src/services/ipc/errors.js，这里保留导出兼容。
export { formatConnectionError } from "../services/ipc/errors";

export function toFiniteOrNull(...values) {
  for (const v of values) {
    if (v === null || v === undefined) continue;
    const n = Number(v);
    if (Number.isFinite(n)) return n;
  }
  return null;
}

export function timestampForFileName(date = new Date()) {
  const p = (n) => String(n).padStart(2, "0");
  return [
    p(date.getMonth() + 1),
    p(date.getDate()),
    p(date.getHours()),
    p(date.getMinutes()),
    p(date.getSeconds()),
  ].join("");
}
