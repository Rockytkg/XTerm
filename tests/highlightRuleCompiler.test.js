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
