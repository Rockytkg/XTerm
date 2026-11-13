import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path) {
  return readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
}

for (const [protocol, path] of [
  ["vnc", "src/components/VncDesktopPane.vue"],
  ["rdp", "src/components/RdpDesktopPane.vue"],
]) {
  test(`${protocol} toolbar actions execute clipboard sending and use application tooltips`, () => {
    const component = source(path);
    const actions = component.match(
      new RegExp(`<div class="${protocol}-toolbar-actions">([\\s\\S]*?)</div>`),
    )?.[1];

    assert.ok(actions);
    assert.match(actions, /@click="sendLocalClipboard"/);
    assert.doesNotMatch(actions, /@click="void sendLocalClipboard"/);
    assert.doesNotMatch(actions, /:title=/);
    assert.equal((actions.match(/<AppTooltip/g) || []).length, 4);
    assert.match(actions, /<component\s+:is="scaleModeIcon"/);
  });
}

test("scale modes have distinct icons", () => {
  const composable = source("src/composables/useRemoteDesktopPane.js");
  assert.match(composable, /fit: Scaling/);
  assert.match(composable, /none: Move/);
  assert.match(composable, /clip: Crop/);
});

test("VNC retries RFB mounting after the template ref is available", () => {
  const component = source("src/components/VncDesktopPane.vue");
  const mounted = component.match(/onMounted\(\(\) => \{([\s\S]*?)\n\}\);/)?.[1];
  assert.ok(mounted);
  assert.match(mounted, /void mountRfb\(\)/);
});

test("VNC fullscreen overrides noVNC inline cursor hiding", () => {
  const component = source("src/components/VncDesktopPane.vue");
  assert.match(component, /&:fullscreen :deep\(canvas\)/);
  assert.match(component, /cursor: default !important/);
});
