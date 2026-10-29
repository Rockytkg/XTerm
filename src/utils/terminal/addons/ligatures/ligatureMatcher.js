export const DEFAULT_LIGATURES = [
  ...new Set([
    "<--",
    "<---",
    "<<-",
    "<-",
    "->",
    "->>",
    "-->",
    "--->",
    "<==",
    "<===",
    "<<=",
    "<=",
    "=>",
    "=>>",
    "==>",
    "===>",
    ">=",
    ">>=",
    "<->",
    "<-->",
    "<--->",
    "<---->",
    "<=>",
    "<==>",
    "<===>",
    "<====>",
    "::",
    ":::",
    "<~~",
    "</",
    "</>",
    "/>",
    "~~>",
    "==",
    "!=",
    "/=",
    "~=",
    "<>",
    "===",
    "!==",
    "!===",
    "<:",
    ":=",
    "*=",
    "*+",
    "<*",
    "<*>",
    "*>",
    "<|",
    "<|>",
    "|>",
    "+*",
    "=*",
    "=:",
    ":>",
    "/*",
    "*/",
    "+++",
    "<!--",
    "<!---",
    "&&",
    "||",
    "??",
    "?.",
    "..",
    "...",
    "..<",
    "..=",
    "++",
    "--",
    "<<",
    "<<<",
    ">>",
    ">>>",
    "=~",
    "!~",
    "~>",
    "<~",
    "||>",
    "<<|",
    "|>>",
    "[|",
    "|]",
    "{|",
    "|}",
    "|-",
    "-|",
    "~-",
    "-~",
    "~~",
    "~~~",
    "##",
    "###",
    "####",
    "#{",
    "#[",
    "#=",
    "#?",
    "#_",
    "#_(",
  ]),
];

const CACHE_LIMIT = 512;

export class LigatureMatcher {
  constructor(ligatures) {
    this._cache = new Map();
    this._byFirstCharacter = new Map();
    const sorted = [...ligatures].sort((a, b) => b.length - a.length);
    for (const ligature of sorted) {
      if (ligature.length < 2) continue;
      const first = ligature[0];
      if (!this._byFirstCharacter.has(first)) this._byFirstCharacter.set(first, []);
      this._byFirstCharacter.get(first).push(ligature);
    }
  }

  findRanges(text) {
    const cached = this._cache.get(text);
    if (cached) return cloneRanges(cached);

    const ranges = [];
    for (let index = 0; index < text.length; index += 1) {
      const candidates = this._byFirstCharacter.get(text[index]);
      if (!candidates) continue;
      for (const ligature of candidates) {
        if (!text.startsWith(ligature, index)) continue;
        ranges.push([index, index + ligature.length]);
        index += ligature.length - 1;
        break;
      }
    }

    if (this._cache.size >= CACHE_LIMIT) {
      this._cache.delete(this._cache.keys().next().value);
    }
    this._cache.set(text, ranges);
    return cloneRanges(ranges);
  }
}

function cloneRanges(ranges) {
  return ranges.map(([start, end]) => [start, end]);
}
