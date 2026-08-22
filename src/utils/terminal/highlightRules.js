import {
  HIGHLIGHT_MATCH_TEXT,
  normalizeHexColor,
  normalizeHighlightMatchType,
} from "../terminalPanelHelpers.js";

export function compileTerminalHighlightRules(rules = [], logger = null) {
  const compiled = [];
  const hashParts = [];
  for (const rule of rules) {
    const color = normalizeHexColor(rule.color);
    if (!rule.pattern || !color) continue;
    const style =
      rule.effect === "background" ? { backgroundColor: color } : { foregroundColor: color };
    const matchType = normalizeHighlightMatchType(rule.matchType);
    if (matchType === HIGHLIGHT_MATCH_TEXT) {
      compiled.push({
        matchType,
        pattern: rule.pattern,
        caseSensitive: !!rule.caseSensitive,
        ...style,
      });
      hashParts.push(highlightRuleHash(rule, matchType, color));
      continue;
    }
    try {
      const flags = `g${rule.caseSensitive ? "" : "i"}`;
      compiled.push({ matchType, regex: new RegExp(rule.pattern, flags), ...style });
      hashParts.push(highlightRegexRuleHash(rule, matchType, flags, color));
    } catch (error) {
      logger?.debug?.("Ignored invalid highlight regex", rule.pattern, error);
    }
  }
  return { rules: compiled, hash: hashParts.join(";") };
}

function highlightRuleHash(rule, matchType, color) {
  return `${matchType}|${rule.pattern}|${rule.caseSensitive ? "cs" : "ci"}|${rule.effect}|${color}`;
}

function highlightRegexRuleHash(rule, matchType, flags, color) {
  return `${matchType}|${rule.pattern}|${flags}|${rule.effect}|${color}`;
}
