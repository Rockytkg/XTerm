function buildCellMap(line, columns) {
  const cells = [];
  let textOffset = 0;
  for (let column = 0; column < columns; column += 1) {
    const cell = line.getCell(column);
    const width = cell?.getWidth() || 0;
    if (width <= 0) continue;
    const text = cell.getChars() || " ";
    cells.push({
      column,
      endOffset: textOffset + text.length,
      startOffset: textOffset,
      width,
    });
    textOffset += text.length;
  }
  return cells;
}

function findCell(cells, textOffset) {
  let low = 0;
  let high = cells.length - 1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const cell = cells[middle];
    if (textOffset < cell.startOffset) high = middle - 1;
    else if (textOffset >= cell.endOffset) low = middle + 1;
    else return cell;
  }
  return null;
}

function toCellRange(cells, match) {
  if (!Number.isFinite(match?.index) || !Number.isFinite(match?.length) || match.length <= 0) {
    return null;
  }
  const first = findCell(cells, match.index);
  if (!first) return null;
  const last = findCell(cells, match.index + match.length - 1) || first;
  return {
    x: first.column,
    width: Math.max(1, last.column + last.width - first.column),
  };
}

function overlaps(left, right) {
  return left.index < right.index + right.length && right.index < left.index + left.length;
}

function decorationKey(range, rule) {
  return [range.x, range.width, rule.foregroundColor || "", rule.backgroundColor || ""].join(":");
}

export function registerLineDecorations({ terminal, marker, line, matches, limit }) {
  const cells = buildCellMap(line, terminal.cols);
  const acceptedMatches = [];
  const decorationKeys = new Set();
  const decorations = [];

  for (const match of matches) {
    if (decorations.length >= limit) break;
    if (acceptedMatches.some((accepted) => overlaps(match, accepted))) continue;

    const range = toCellRange(cells, match);
    if (!range || range.x < 0 || range.x >= terminal.cols) continue;
    range.width = Math.min(range.width, terminal.cols - range.x);
    if (range.width <= 0) continue;

    acceptedMatches.push(match);
    const key = decorationKey(range, match.rule);
    if (decorationKeys.has(key)) continue;
    decorationKeys.add(key);

    const decoration = terminal.registerDecoration({
      marker,
      x: range.x,
      width: range.width,
      foregroundColor: match.rule.foregroundColor || undefined,
      backgroundColor: match.rule.backgroundColor || undefined,
      layer: "top",
    });
    if (decoration) decorations.push(decoration);
  }

  return decorations;
}
