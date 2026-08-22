import assert from "node:assert/strict";
import test from "node:test";
import { registerLineDecorations } from "../src/utils/terminal/addons/highlight/lineDecorations.js";

function cell(chars, width) {
  return {
    getChars: () => chars,
    getWidth: () => width,
  };
}

test("line decorations map wide and combining characters to terminal columns", () => {
  const registered = [];
  const terminal = {
    cols: 5,
    registerDecoration: (options) => {
      registered.push(options);
      return { dispose() {} };
    },
  };
  const line = {
    getCell: (column) =>
      [cell("A", 1), cell("你", 2), cell("", 0), cell("é", 1), cell("B", 1)][column],
  };
  const rule = { foregroundColor: "#ff0000" };

  registerLineDecorations({
    terminal,
    marker: {},
    line,
    matches: [
      { index: 1, length: 1, rule },
      { index: 2, length: 2, rule },
    ],
    limit: 32,
  });

  assert.deepEqual(
    registered.map(({ x, width }) => ({ x, width })),
    [
      { x: 1, width: 2 },
      { x: 3, width: 1 },
    ],
  );
});
