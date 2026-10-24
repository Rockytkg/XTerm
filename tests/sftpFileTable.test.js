import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { computed, ref } from "vue";

const source = readFileSync(
  new URL("../src/components/sftp/SftpFileTable.vue", import.meta.url),
  "utf8",
);
const visibleRangeMatch = source.match(
  /const visibleRange = computed\(\(\) => \{([\s\S]*?)\r?\n\}\);/,
);
assert.ok(visibleRangeMatch, "visibleRange implementation not found");

const createVisibleRange = new Function(
  "computed",
  "rows",
  "shouldVirtualize",
  "scrollTop",
  "viewportHeight",
  "ROW_HEIGHT",
  "OVERSCAN_ROWS",
  `return computed(() => {${visibleRangeMatch[1]}\n});`,
);

test("虚拟列表从 1000 行收缩到 100 行时 clamp 滚动起点且保留可见行", () => {
  const rows = ref(Array.from({ length: 1000 }));
  const scrollTop = ref(1000 * 34);
  const viewportHeight = ref(340);
  const shouldVirtualize = computed(() => rows.value.length > 80);
  const visibleRange = createVisibleRange(
    computed,
    rows,
    shouldVirtualize,
    scrollTop,
    viewportHeight,
    34,
    8,
  );

  assert.equal(visibleRange.value.end, 1000);
  rows.value = Array.from({ length: 100 });

  assert.deepEqual(visibleRange.value, {
    start: 75,
    end: 100,
    top: 75 * 34,
    bottom: 0,
  });
  assert.ok(visibleRange.value.start < visibleRange.value.end);
});
