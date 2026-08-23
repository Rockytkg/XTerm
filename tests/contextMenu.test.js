import assert from "node:assert/strict";
import test from "node:test";
import { isContextMenuDangerItem, shouldUseDomContextMenu } from "../src/utils/contextMenu.js";

test("DOM context menu fallback applies to Linux Wayland sessions only", () => {
  assert.equal(shouldUseDomContextMenu({ platform: "linux", session: "wayland" }), true);
  assert.equal(shouldUseDomContextMenu({ platform: "linux", session: "x11" }), false);
  assert.equal(shouldUseDomContextMenu({ platform: "linux", session: "unknown" }), false);
  assert.equal(shouldUseDomContextMenu({ platform: "windows", session: "native" }), false);
  assert.equal(shouldUseDomContextMenu({ platform: "macos", session: "native" }), false);
});

test("DOM context menu fallback tolerates missing environment info", () => {
  assert.equal(shouldUseDomContextMenu(undefined), false);
  assert.equal(shouldUseDomContextMenu(null), false);
  assert.equal(shouldUseDomContextMenu({}), false);
});

test("danger menu items are detected by tone or delete-ish id", () => {
  assert.equal(isContextMenuDangerItem({ id: "sftp-delete" }), true);
  assert.equal(isContextMenuDangerItem({ id: "custom", tone: "danger" }), true);
  assert.equal(isContextMenuDangerItem({ id: "sftp-refresh" }), false);
  assert.equal(isContextMenuDangerItem(null), false);
  assert.equal(isContextMenuDangerItem(undefined), false);
});
