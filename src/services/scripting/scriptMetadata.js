// 脚本头元数据（油猴 ==UserScript== 风格）：脚本代码即唯一事实来源，
// 名称/作者/主页/备注/版本/更新地址都从 ==XTermScript== 头块解析，
// 新建时按模板生成，导入/编辑后重新解析同步。

export const SCRIPT_HEADER_OPEN = "// ==XTermScript==";
export const SCRIPT_HEADER_CLOSE = "// ==/XTermScript==";

const HEADER_KEYS = Object.freeze({
  name: "name",
  author: "author",
  homepage: "homepage",
  description: "description",
  version: "version",
  updateurl: "updateUrl",
});

const HEADER_LINE_RE = /^\/\/\s*@([a-zA-Z]+)\s+(.*\S)\s*$/;

export const EMPTY_SCRIPT_METADATA = Object.freeze({
  name: "",
  author: "",
  homepage: "",
  description: "",
  version: "",
  updateUrl: "",
});

export function parseScriptMetadata(code) {
  const metadata = { ...EMPTY_SCRIPT_METADATA };
  const text = String(code || "");
  const openIndex = text.indexOf(SCRIPT_HEADER_OPEN);
  if (openIndex < 0) return metadata;
  const closeIndex = text.indexOf(SCRIPT_HEADER_CLOSE, openIndex + SCRIPT_HEADER_OPEN.length);
  if (closeIndex < 0) return metadata;

  const block = text.slice(openIndex + SCRIPT_HEADER_OPEN.length, closeIndex);
  for (const line of block.split(/\r?\n/)) {
    const match = HEADER_LINE_RE.exec(line.trim());
    if (!match) continue;
    const key = HEADER_KEYS[match[1].toLowerCase()];
    if (key) metadata[key] = match[2].trim();
  }
  return metadata;
}

export function buildScriptHeader(metadata = {}) {
  const lines = [SCRIPT_HEADER_OPEN];
  const push = (key, value) => {
    const text = String(value || "").trim();
    if (text) lines.push(`// @${key.padEnd(11)}${text}`);
  };
  push("name", metadata.name);
  push("author", metadata.author);
  push("homepage", metadata.homepage);
  push("description", metadata.description);
  push("version", metadata.version || "1.0.0");
  push("updateURL", metadata.updateUrl);
  lines.push(SCRIPT_HEADER_CLOSE);
  return lines.join("\n");
}

// 宽松的数字段版本比较：1.10.0 > 1.2.9；无法解析的段按 0 处理。
export function compareScriptVersions(a, b) {
  const toParts = (value) =>
    String(value || "")
      .split(".")
      .map((part) => {
        const num = Number.parseInt(part, 10);
        return Number.isFinite(num) ? num : 0;
      });
  const left = toParts(a);
  const right = toParts(b);
  const length = Math.max(left.length, right.length);
  for (let i = 0; i < length; i += 1) {
    const diff = (left[i] || 0) - (right[i] || 0);
    if (diff !== 0) return diff < 0 ? -1 : 1;
  }
  return 0;
}
