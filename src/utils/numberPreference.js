// 数字偏好归一化：非有限值回退到 fallback，否则夹取到 [min, max]，integer 时取整。
export function normalizeNumberPreference(target, key, fallback, min, max, integer = false) {
  const value = Number(target[key]);
  if (!Number.isFinite(value)) {
    target[key] = fallback;
    return;
  }
  const clamped = Math.min(max, Math.max(min, value));
  target[key] = integer ? Math.round(clamped) : clamped;
}
