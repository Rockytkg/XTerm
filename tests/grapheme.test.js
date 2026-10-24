import assert from "node:assert/strict";
import test from "node:test";

import {
  graphemeSegments,
  sliceGraphemePrefix,
  sliceGraphemeSuffix,
} from "../src/utils/grapheme.js";

test("grapheme iteration keeps combining and ZWJ sequences together", () => {
  assert.deepEqual([...graphemeSegments("e\u0301👨‍👩‍👧‍👦x")], ["e\u0301", "👨‍👩‍👧‍👦", "x"]);
});

test("prefix slicing preserves the UTF-16 budget without splitting a grapheme", () => {
  const title = `${"a".repeat(21)}🙂x`;
  const truncated = `${sliceGraphemePrefix(title, 22)}…`;
  assert.equal(truncated, `${"a".repeat(21)}…`);
  assert.equal(truncated.isWellFormed(), true);
  assert.doesNotThrow(() => encodeURIComponent(truncated));
  assert.equal(sliceGraphemePrefix("e\u0301x", 1), "");
  assert.equal(sliceGraphemePrefix("e\u0301x", 2), "e\u0301");
  assert.equal(sliceGraphemePrefix("👨‍👩‍👧‍👦x", 2), "");
  assert.equal(sliceGraphemePrefix("abc", 0), "");
});

test("suffix slicing starts on a complete grapheme boundary", () => {
  const lowBoundary = `${"a".repeat(65536)}🙂${"b".repeat(65535)}`;
  const lowResult = sliceGraphemeSuffix(lowBoundary, 65536);
  assert.equal(lowResult, "b".repeat(65535));
  assert.equal(lowResult.isWellFormed(), true);

  const highBoundary = `${"a".repeat(65537)}🙂${"b".repeat(65534)}`;
  const highResult = sliceGraphemeSuffix(highBoundary, 65536);
  assert.equal(highResult, `🙂${"b".repeat(65534)}`);
  assert.equal(highResult.isWellFormed(), true);

  assert.equal(sliceGraphemeSuffix("x👨‍👩‍👧‍👦y", 12), "👨‍👩‍👧‍👦y");
  assert.equal(sliceGraphemeSuffix("x👨‍👩‍👧‍👦y", 11), "y");
  assert.equal(sliceGraphemeSuffix("abc", 0), "");
});
