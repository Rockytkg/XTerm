const MATCH_TEXT = "text";
const INDEX_OF_RULE_LIMIT = 2;
const PREFIX_SCAN_RULE_LIMIT = 8;
const PREFIX_SCAN_TOTAL_LENGTH_LIMIT = 96;
const REGEX_LITERAL_MIN_LENGTH = 3;
const MATCH_CANDIDATE_MULTIPLIER = 4;

function compareMatches(left, right) {
  return (
    left.index - right.index ||
    right.length - left.length ||
    (left.rule?.priority ?? 0) - (right.rule?.priority ?? 0)
  );
}

function splitTopLevelAlternatives(source) {
  const parts = [];
  let depth = 0;
  let bracketDepth = 0;
  let escaped = false;
  let current = "";

  for (let index = 0; index < source.length; index += 1) {
    const ch = source[index];
    if (escaped) {
      current += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\") {
      current += ch;
      escaped = true;
      continue;
    }
    if (ch === "[" && bracketDepth === 0) {
      bracketDepth = 1;
      current += ch;
      continue;
    }
    if (ch === "]" && bracketDepth > 0) {
      bracketDepth -= 1;
      current += ch;
      continue;
    }
    if (bracketDepth > 0) {
      current += ch;
      continue;
    }
    if (ch === "(") {
      depth += 1;
      current += ch;
      continue;
    }
    if (ch === ")") {
      depth = Math.max(0, depth - 1);
      current += ch;
      continue;
    }
    if (ch === "|" && depth === 0) {
      parts.push(current);
      current = "";
      continue;
    }
    current += ch;
  }

  if (current) parts.push(current);
  return parts;
}

function decodeRegexLiteral(source) {
  let literal = "";
  let escaped = false;
  for (let index = 0; index < source.length; index += 1) {
    const ch = source[index];
    if (escaped) {
      if (/^[dDsSwWbBAZzGkpPqrRtxuvfn0-9]$/.test(ch)) return null;
      literal += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\") {
      escaped = true;
      continue;
    }
    if ("()[]{}*+?|^$.".includes(ch)) return null;
    literal += ch;
  }
  if (escaped) return null;
  return literal;
}

function extractAlternationLiterals(source) {
  const wrapped = source.match(
    /^(?:\\b|\(\?<![^)]*\)|\^)?\(\?:([\s\S]+)\)(?:\\b|\(\?![^)]*\)|\$)?$/,
  );
  if (!wrapped) return null;

  const alternatives = splitTopLevelAlternatives(wrapped[1]);
  if (alternatives.length < 2) return null;

  const literals = [];
  for (const alternative of alternatives) {
    const literal = decodeRegexLiteral(alternative);
    if (!literal || literal.length < REGEX_LITERAL_MIN_LENGTH) return null;
    literals.push(literal);
  }
  return literals;
}

function extractLiteralPrefix(source) {
  let index = 0;
  while (index < source.length) {
    if (source.startsWith("\\b", index) || source.startsWith("^", index)) {
      index += source[index] === "^" ? 1 : 2;
      continue;
    }
    if (source.startsWith("(?<!", index) || source.startsWith("(?<=", index)) {
      const end = source.indexOf(")", index);
      if (end < 0) break;
      index = end + 1;
      continue;
    }
    break;
  }

  let literal = "";
  let escaped = false;
  for (; index < source.length; index += 1) {
    const ch = source[index];
    if (escaped) {
      if (/^[dDsSwWbBAZzGkpPqrRtxuvfn0-9]$/.test(ch)) break;
      literal += ch;
      escaped = false;
      continue;
    }
    if (ch === "\\") {
      escaped = true;
      continue;
    }
    if ("()[]{}*+?|^$.".includes(ch)) break;
    literal += ch;
  }

  return literal.length >= REGEX_LITERAL_MIN_LENGTH ? literal : null;
}

function buildRegexPrefilter(rule) {
  const source = rule?.regex?.source;
  if (!source) return null;

  const alternationLiterals = extractAlternationLiterals(source);
  if (alternationLiterals?.length) {
    return {
      literals: alternationLiterals.map((literal) =>
        rule.regex.ignoreCase ? literal.toLocaleLowerCase() : literal,
      ),
      ignoreCase: rule.regex.ignoreCase,
    };
  }

  const literalPrefix = extractLiteralPrefix(source);
  if (!literalPrefix) return null;
  return {
    literals: [rule.regex.ignoreCase ? literalPrefix.toLocaleLowerCase() : literalPrefix],
    ignoreCase: rule.regex.ignoreCase,
  };
}

function optimizeRegexRule(rule) {
  if (!rule?.regex) return rule;
  const prefilter = buildRegexPrefilter(rule);
  return prefilter ? { ...rule, prefilter } : rule;
}

// trie 构建 + BFS fail 链对两种 matcher 完全一致，共享一份 nodes；
// 差异只在 search 的输出形态（带 limit 的匹配列表 vs 候选规则集合）。
function buildAhoCorasickNodes(patterns) {
  const nodes = [{ next: new Map(), fail: 0, out: [] }];

  function addPattern(pattern, payload) {
    let state = 0;
    for (let i = 0; i < pattern.length; i += 1) {
      const ch = pattern[i];
      const next = nodes[state].next.get(ch);
      if (next !== undefined) {
        state = next;
        continue;
      }
      const id = nodes.length;
      nodes.push({ next: new Map(), fail: 0, out: [] });
      nodes[state].next.set(ch, id);
      state = id;
    }
    nodes[state].out.push(payload);
  }

  for (const p of patterns) {
    if (p?.pattern) addPattern(p.pattern, p.payload);
  }

  const queue = [];
  for (const [, next] of nodes[0].next) {
    nodes[next].fail = 0;
    queue.push(next);
  }
  for (let qi = 0; qi < queue.length; qi += 1) {
    const r = queue[qi];
    for (const [ch, s] of nodes[r].next) {
      queue.push(s);
      let f = nodes[r].fail;
      while (f && !nodes[f].next.has(ch)) f = nodes[f].fail;
      const fallback = nodes[f].next.get(ch);
      nodes[s].fail = fallback !== undefined ? fallback : 0;
      nodes[s].out = nodes[s].out.concat(nodes[nodes[s].fail].out);
    }
  }

  return nodes;
}

function buildAhoCorasick(patterns) {
  const nodes = buildAhoCorasickNodes(patterns);

  return {
    search(text, output, limit) {
      let state = 0;
      for (let i = 0; i < text.length && output.length < limit; i += 1) {
        const ch = text[i];
        while (state && !nodes[state].next.has(ch)) state = nodes[state].fail;
        const next = nodes[state].next.get(ch);
        state = next !== undefined ? next : 0;
        for (const out of nodes[state].out) {
          output.push({ index: i - out.length + 1, length: out.length, rule: out.rule });
          if (output.length >= limit) break;
        }
      }
    },
  };
}

function buildRuleCandidateMatcher(patterns) {
  const nodes = buildAhoCorasickNodes(patterns);

  return {
    search(text, results) {
      let state = 0;
      for (let index = 0; index < text.length; index += 1) {
        const ch = text[index];
        while (state && !nodes[state].next.has(ch)) state = nodes[state].fail;
        const next = nodes[state].next.get(ch);
        state = next !== undefined ? next : 0;
        for (const out of nodes[state].out) results.add(out.rule);
      }
    },
  };
}

function collectPatternMatches(source, pattern, rule, output, limit) {
  let fromIndex = 0;
  while (pattern && output.length < limit) {
    const index = source.indexOf(pattern, fromIndex);
    if (index < 0) break;
    output.push({ index, length: pattern.length, rule });
    fromIndex = index + Math.max(1, pattern.length);
  }
}

class IndexOfLiteralMatcher {
  constructor(rules) {
    this.rules = rules;
  }

  search(text, output, limit) {
    for (const rule of this.rules) {
      collectPatternMatches(text, rule.pattern, rule.rule, output, limit);
      if (output.length >= limit) break;
    }
  }
}

class PrefixLiteralMatcher {
  constructor(rules) {
    this.buckets = new Map();
    for (const rule of rules) {
      const firstChar = rule.pattern[0];
      const bucket = this.buckets.get(firstChar);
      if (bucket) bucket.push(rule);
      else this.buckets.set(firstChar, [rule]);
    }
    for (const bucket of this.buckets.values()) {
      bucket.sort((left, right) => right.pattern.length - left.pattern.length);
    }
  }

  search(text, output, limit) {
    for (let index = 0; index < text.length && output.length < limit; index += 1) {
      const bucket = this.buckets.get(text[index]);
      if (!bucket) continue;
      for (const rule of bucket) {
        if (!text.startsWith(rule.pattern, index)) continue;
        output.push({ index, length: rule.pattern.length, rule: rule.rule });
        if (output.length >= limit) break;
      }
    }
  }
}

class AhoCorasickLiteralMatcher {
  constructor(rules) {
    this.matcher = buildAhoCorasick(
      rules.map((rule) => ({
        pattern: rule.pattern,
        payload: { length: rule.pattern.length, rule: rule.rule },
      })),
    );
  }

  search(text, output, limit) {
    this.matcher.search(text, output, limit);
  }
}

class RegexRuleMatcher {
  constructor(rules) {
    const caseSensitivePatterns = [];
    const caseInsensitivePatterns = [];
    this.fallbackRules = [];

    for (const rule of rules) {
      const literals = rule.prefilter?.literals;
      if (!literals?.length) {
        this.fallbackRules.push(rule);
        continue;
      }
      const target = rule.prefilter.ignoreCase ? caseInsensitivePatterns : caseSensitivePatterns;
      for (const literal of literals) {
        target.push({
          pattern: literal,
          payload: { rule },
        });
      }
    }

    this.caseSensitiveMatcher = caseSensitivePatterns.length
      ? buildRuleCandidateMatcher(caseSensitivePatterns)
      : null;
    this.caseInsensitiveMatcher = caseInsensitivePatterns.length
      ? buildRuleCandidateMatcher(caseInsensitivePatterns)
      : null;
  }

  static #resolveMatchSpan(match) {
    if (!match?.[0]) return null;
    const full = match[0];
    const firstCapture = match[1];
    if (!firstCapture || firstCapture.length === 0 || firstCapture.length >= full.length) {
      return { index: match.index, length: full.length };
    }
    const relativeIndex = full.indexOf(firstCapture);
    if (relativeIndex < 0) return { index: match.index, length: full.length };
    return {
      index: match.index + relativeIndex,
      length: firstCapture.length,
    };
  }

  get hasCaseInsensitiveRules() {
    return this.caseInsensitiveMatcher !== null;
  }

  collect(text, lowercasedText, output, limit) {
    const candidateRules = new Set();
    this.caseSensitiveMatcher?.search(text, candidateRules);
    if (this.caseInsensitiveMatcher) {
      // 小写文本由 collectMatches 统一计算并下传；直接调用时兜底现算
      this.caseInsensitiveMatcher.search(
        lowercasedText ?? text.toLocaleLowerCase(),
        candidateRules,
      );
    }
    for (const rule of this.fallbackRules) candidateRules.add(rule);

    const rules = Array.from(candidateRules).sort(
      (left, right) => (left.priority ?? 0) - (right.priority ?? 0),
    );
    for (const rule of rules) {
      rule.regex.lastIndex = 0;
      let match;
      while ((match = rule.regex.exec(text)) && output.length < limit) {
        const span = RegexRuleMatcher.#resolveMatchSpan(match);
        if (span) {
          output.push({ index: span.index, length: span.length, rule });
        } else {
          rule.regex.lastIndex += 1;
        }
      }
      if (output.length >= limit) break;
    }
  }
}

class CompiledHighlightRuleSet {
  constructor({ hash, ruleCount, regexRules, textCaseSensitive, textCaseInsensitive }) {
    this.hash = hash;
    this.ruleCount = ruleCount;
    this.textMatcherCaseSensitive = this.#buildLiteralMatcher(textCaseSensitive);
    this.textMatcherCaseInsensitive = this.#buildLiteralMatcher(textCaseInsensitive);
    this.regexRuleMatcher = new RegexRuleMatcher(regexRules);
  }

  collectMatches(text, limit) {
    if (limit <= 0) return [];
    const candidateLimit = limit * MATCH_CANDIDATE_MULTIPLIER;
    const candidates = [];
    const collectLiteralMatches = (matcher, source) => {
      if (!matcher) return;
      const matches = [];
      matcher.search(source, matches, candidateLimit);
      candidates.push(...matches);
    };

    // 大小写不敏感的 literal 与 regex 预筛复用同一份小写文本，避免每行重复 toLocaleLowerCase；
    // 两类 matcher 都不存在时不计算（空行/全大小写敏感规则的开销为零）
    const lowercasedText =
      this.textMatcherCaseInsensitive || this.regexRuleMatcher.hasCaseInsensitiveRules
        ? text.toLocaleLowerCase()
        : null;

    collectLiteralMatches(this.textMatcherCaseSensitive, text);
    collectLiteralMatches(this.textMatcherCaseInsensitive, lowercasedText);
    const regexMatches = [];
    this.regexRuleMatcher.collect(text, lowercasedText, regexMatches, candidateLimit);
    candidates.push(...regexMatches);
    candidates.sort(compareMatches);
    return candidates.slice(0, limit);
  }

  #buildLiteralMatcher(rules) {
    if (rules.length === 0) return null;
    if (rules.length <= INDEX_OF_RULE_LIMIT) return new IndexOfLiteralMatcher(rules);

    const totalPatternLength = rules.reduce((sum, rule) => sum + rule.pattern.length, 0);
    if (
      rules.length <= PREFIX_SCAN_RULE_LIMIT &&
      totalPatternLength <= PREFIX_SCAN_TOTAL_LENGTH_LIMIT
    ) {
      return new PrefixLiteralMatcher(rules);
    }

    return new AhoCorasickLiteralMatcher(rules);
  }
}

function splitTextPattern(pattern) {
  return String(pattern || "")
    .split("|")
    .map((item) => item.trim())
    .filter(Boolean);
}

function textRuleDedupeKey(pattern, rule) {
  const normalizedPattern = rule.caseSensitive ? pattern : pattern.toLocaleLowerCase();
  return [
    normalizedPattern,
    rule.caseSensitive ? "cs" : "ci",
    rule.foregroundColor || "",
    rule.backgroundColor || "",
  ].join("|");
}

function expandHighlightRules(rules) {
  if (!Array.isArray(rules)) return [];
  const expanded = [];
  const textKeys = new Set();
  for (const rule of rules) {
    if (!rule) continue;
    if (rule.matchType !== MATCH_TEXT) {
      expanded.push(rule);
      continue;
    }
    for (const pattern of splitTextPattern(rule.pattern)) {
      const key = textRuleDedupeKey(pattern, rule);
      if (textKeys.has(key)) continue;
      textKeys.add(key);
      expanded.push({ ...rule, pattern });
    }
  }
  return expanded;
}

export function compileHighlightRuleSet(rules, hash) {
  const compiledRules = expandHighlightRules(rules).map((rule, index) => ({
    ...rule,
    priority: index,
  }));
  const textCaseSensitive = [];
  const textCaseInsensitive = [];
  const regexRules = [];

  for (const rule of compiledRules) {
    if (rule?.matchType !== MATCH_TEXT) {
      if (rule?.regex) regexRules.push(optimizeRegexRule(rule));
      continue;
    }

    const pattern = String(rule.pattern || "");
    if (!pattern) continue;
    if (rule.caseSensitive) textCaseSensitive.push({ pattern, rule });
    else textCaseInsensitive.push({ pattern: pattern.toLocaleLowerCase(), rule });
  }

  return new CompiledHighlightRuleSet({
    hash,
    ruleCount: compiledRules.length,
    regexRules,
    textCaseSensitive,
    textCaseInsensitive,
  });
}
