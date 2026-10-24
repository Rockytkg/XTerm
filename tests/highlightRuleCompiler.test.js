import assert from "node:assert/strict";
import test from "node:test";
import { compileHighlightRuleSet } from "../src/utils/terminal/addons/highlight/ruleCompiler.js";

test("dense text matches do not starve regex highlight rules", () => {
  const rules = [
    {
      caseSensitive: true,
      foregroundColor: "#ffffff",
      matchType: "text",
      pattern: "x",
    },
    {
      backgroundColor: "#ff0000",
      matchType: "regex",
      regex: /ALERT/g,
    },
  ];
  const compiled = compileHighlightRuleSet(rules, "mixed-rules");
  const matches = compiled.collectMatches(`ALERT ${"x".repeat(80)}`, 32);

  assert.equal(
    matches.some((match) => match.index === 0 && match.length === 5),
    true,
  );
  assert.equal(matches.length, 32);
});

function compileCaseInsensitiveLiteral(pattern, ruleCount = 1) {
  const patterns = [pattern, ...Array.from({ length: ruleCount - 1 }, (_, index) => `q${index}`)];
  return compileHighlightRuleSet(
    patterns.map((item) => ({
      caseSensitive: false,
      matchType: "text",
      pattern: item,
    })),
    "case-insensitive-literals",
  );
}

function matchSpans(matches) {
  return matches.map(({ index, length }) => ({ index, length }));
}

for (const ruleCount of [1, 3, 10]) {
  test(`case-insensitive literals use original UTF-16 spans with ${ruleCount} rules`, () => {
    const compiled = compileCaseInsensitiveLiteral("b", ruleCount);

    assert.deepEqual(matchSpans(compiled.collectMatches("AİB", 32)), [{ index: 2, length: 1 }]);
    assert.deepEqual(matchSpans(compiled.collectMatches("BİBİB", 32)), [
      { index: 0, length: 1 },
      { index: 2, length: 1 },
      { index: 4, length: 1 },
    ]);

    for (const pattern of ["i", "\u0307", "i\u0307"]) {
      const expansionCompiled = compileCaseInsensitiveLiteral(pattern, ruleCount);
      assert.deepEqual(matchSpans(expansionCompiled.collectMatches("AİB", 32)), [
        { index: 1, length: 1 },
      ]);
    }
  });
}

test("case-insensitive literals retain context-sensitive equal-length lowercase matches", () => {
  const compiled = compileCaseInsensitiveLiteral("ς");

  assert.deepEqual(matchSpans(compiled.collectMatches("AΣ", 32)), [{ index: 1, length: 1 }]);
});

test("case-insensitive literals discard matches when lowercase offsets cannot be aligned", (t) => {
  const compiled = compileCaseInsensitiveLiteral("b");
  const originalToLocaleLowerCase = String.prototype.toLocaleLowerCase;
  String.prototype.toLocaleLowerCase = function () {
    if (this.valueOf() === "AB") return "b";
    return originalToLocaleLowerCase.call(this);
  };
  t.after(() => {
    String.prototype.toLocaleLowerCase = originalToLocaleLowerCase;
  });

  assert.deepEqual(compiled.collectMatches("AB", 32), []);
});
