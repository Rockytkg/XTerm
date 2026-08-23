import assert from "node:assert/strict";
import test from "node:test";
import { isContextMenuDangerItem } from "../src/utils/contextMenu.js";

test("danger menu items are detected by tone or delete-ish id", () => {
  assert.equal(isContextMenuDangerItem({ id: "sftp-delete" }), true);
  assert.equal(isContextMenuDangerItem({ id: "custom", tone: "danger" }), true);
  assert.equal(isContextMenuDangerItem({ id: "sftp-refresh" }), false);
  assert.equal(isContextMenuDangerItem(null), false);
  assert.equal(isContextMenuDangerItem(undefined), false);
});
