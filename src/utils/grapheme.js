const graphemeSegmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });

function codeUnitBudget(value) {
  return Math.max(0, Math.floor(Number(value) || 0));
}

export function* graphemeSegments(value) {
  const text = String(value ?? "");
  for (const { segment } of graphemeSegmenter.segment(text)) yield segment;
}

export function sliceGraphemePrefix(value, maxCodeUnits) {
  const text = String(value ?? "");
  const limit = codeUnitBudget(maxCodeUnits);
  if (text.length <= limit) return text;

  let end = 0;
  for (const segment of graphemeSegments(text)) {
    if (end + segment.length > limit) break;
    end += segment.length;
  }
  return text.slice(0, end);
}

export function sliceGraphemeSuffix(value, maxCodeUnits) {
  const text = String(value ?? "");
  const limit = codeUnitBudget(maxCodeUnits);
  if (text.length <= limit) return text;

  const start = text.length - limit;
  let offset = 0;
  let safeStart = text.length;
  for (const segment of graphemeSegments(text)) {
    const end = offset + segment.length;
    if (end <= start) {
      offset = end;
      continue;
    }
    safeStart = offset >= start ? offset : end;
    break;
  }
  return text.slice(safeStart);
}
