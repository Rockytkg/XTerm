import assert from "node:assert/strict";
import test from "node:test";
import {
  CONTEXT_MENU_LAYOUT,
  contextMenuHeight,
  contextMenuPosition,
  isContextMenuActivatable,
  isContextMenuDangerItem,
  nextActivatableMenuIndex,
  normalizeContextMenuItems,
  typeaheadMenuIndex,
} from "../src/utils/contextMenu.js";

test("danger menu items are detected by tone or delete-ish id", () => {
  assert.equal(isContextMenuDangerItem({ id: "sftp-delete" }), true);
  assert.equal(isContextMenuDangerItem({ id: "custom", tone: "danger" }), true);
  assert.equal(isContextMenuDangerItem({ id: "sftp-refresh" }), false);
  assert.equal(isContextMenuDangerItem(null), false);
  assert.equal(isContextMenuDangerItem(undefined), false);
});

test("normalize flattens nested arrays and drops empty entries", () => {
  const items = normalizeContextMenuItems([[{ id: "a" }], null, [{ id: "b" }]]);
  assert.deepEqual(
    items.map((item) => item.id),
    ["a", "b"],
  );
});

test("normalize keeps preserved disabled global edit items, drops other disabled items", () => {
  const items = normalizeContextMenuItems([
    { id: "global-paste", enabled: false },
    { id: "custom", enabled: false },
    { id: "visible" },
  ]);
  assert.deepEqual(
    items.map((item) => item.id),
    ["global-paste", "visible"],
  );
  assert.equal(items[0].enabled, false);
});

test("normalize collapses duplicate separators and trims edges", () => {
  const items = normalizeContextMenuItems([
    { type: "separator" },
    { id: "a" },
    { type: "separator" },
    { type: "separator" },
    { id: "b" },
    { type: "separator" },
  ]);
  assert.deepEqual(
    items.map((item) => item.type),
    ["item", "separator", "item"],
  );
});

test("normalize assigns fallback ids and defaults enabled to true", () => {
  const items = normalizeContextMenuItems([{ label: "x" }]);
  assert.equal(items[0].id, "item-0");
  assert.equal(items[0].enabled, true);
  assert.equal(items[0].type, "item");
});

test("menu height clamps to min and max", () => {
  assert.equal(contextMenuHeight([]), CONTEXT_MENU_LAYOUT.minHeight);
  const manyItems = Array.from({ length: 100 }, (_, index) => ({ id: `i-${index}` }));
  assert.equal(contextMenuHeight(manyItems), CONTEXT_MENU_LAYOUT.maxHeight);
});

test("menu height sums items and separators", () => {
  const height = contextMenuHeight([{ id: "a" }, { type: "separator" }, { id: "b" }]);
  assert.equal(
    height,
    CONTEXT_MENU_LAYOUT.verticalPadding +
      CONTEXT_MENU_LAYOUT.itemHeight * 2 +
      CONTEXT_MENU_LAYOUT.separatorHeight,
  );
});

test("activatable items exclude separators and disabled entries", () => {
  assert.equal(isContextMenuActivatable({ type: "item", enabled: true }), true);
  assert.equal(isContextMenuActivatable({ type: "item", enabled: false }), false);
  assert.equal(isContextMenuActivatable({ type: "separator" }), false);
  assert.equal(isContextMenuActivatable(null), false);
});

test("next activatable index skips disabled items and separators, wrapping around", () => {
  const items = [
    { type: "item", enabled: false },
    { type: "item" },
    { type: "separator" },
    { type: "item" },
  ];
  assert.equal(nextActivatableMenuIndex(items, -1, 1), 1);
  assert.equal(nextActivatableMenuIndex(items, 1, 1), 3);
  assert.equal(nextActivatableMenuIndex(items, 3, 1), 1);
  assert.equal(nextActivatableMenuIndex(items, 1, -1), 3);
  assert.equal(nextActivatableMenuIndex(items, items.length, -1), 3);
});

test("next activatable index returns -1 when nothing is activatable", () => {
  assert.equal(nextActivatableMenuIndex([], 0, 1), -1);
  assert.equal(
    nextActivatableMenuIndex([{ type: "item", enabled: false }, { type: "separator" }], 0, 1),
    -1,
  );
});

test("typeahead jumps to next item whose label starts with the char", () => {
  const items = [
    { type: "item", label: "Copy" },
    { type: "item", label: "Cut", enabled: false },
    { type: "item", label: "Paste" },
    { type: "item", label: "Clear" },
  ];
  // 大小写不敏感；禁用项（Cut）被跳过；从当前位置之后继续找并环绕。
  assert.equal(typeaheadMenuIndex(items, -1, "c"), 0);
  assert.equal(typeaheadMenuIndex(items, 0, "C"), 3);
  assert.equal(typeaheadMenuIndex(items, 3, "p"), 2);
  assert.equal(typeaheadMenuIndex(items, 0, "z"), -1);
});

test("menu position stays below-right of the pointer when space allows", () => {
  const { x, y } = contextMenuPosition({
    x: 100,
    y: 100,
    width: 180,
    height: 200,
    viewWidth: 1024,
    viewHeight: 768,
  });
  assert.equal(x, 100);
  assert.equal(y, 100);
});

test("menu position flips to the other side of the pointer near viewport edges", () => {
  const { x, y } = contextMenuPosition({
    x: 1000,
    y: 740,
    width: 180,
    height: 200,
    viewWidth: 1024,
    viewHeight: 768,
  });
  assert.equal(x, 1000 - 180);
  assert.equal(y, 740 - 200);
});

test("menu position clamps into the safe margin on tiny viewports", () => {
  const { x, y } = contextMenuPosition({
    x: 5,
    y: 5,
    width: 180,
    height: 540,
    viewWidth: 150,
    viewHeight: 300,
  });
  assert.equal(x, CONTEXT_MENU_LAYOUT.screenMargin);
  assert.equal(y, CONTEXT_MENU_LAYOUT.screenMargin);
});
