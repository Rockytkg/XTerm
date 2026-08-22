import assert from "node:assert/strict";
import test from "node:test";
import {
  buildScriptHeader,
  compareScriptVersions,
  parseScriptMetadata,
} from "../src/services/scripting/scriptMetadata.js";

const SAMPLE = `// ==XTermScript==
// @name        交换机开局
// @author      liushicong
// @homepage    https://example.com/u/me
// @description 批量开局配置
// @version     1.2.3
// @updateURL   https://example.com/script.js
// ==/XTermScript==

await xterm.sendLine("system-view");
`;

test("parseScriptMetadata reads all header fields", () => {
  const meta = parseScriptMetadata(SAMPLE);
  assert.equal(meta.name, "交换机开局");
  assert.equal(meta.author, "liushicong");
  assert.equal(meta.homepage, "https://example.com/u/me");
  assert.equal(meta.description, "批量开局配置");
  assert.equal(meta.version, "1.2.3");
  assert.equal(meta.updateUrl, "https://example.com/script.js");
});

test("parseScriptMetadata returns empty metadata without a header", () => {
  const meta = parseScriptMetadata(`await xterm.sleep(1);`);
  assert.equal(meta.name, "");
  assert.equal(meta.version, "");
  assert.equal(meta.updateUrl, "");
});

test("buildScriptHeader always includes a version and skips empty fields", () => {
  const header = buildScriptHeader({ name: "demo" });
  assert.match(header, /@name\s+demo/);
  assert.match(header, /@version\s+1\.0\.0/);
  assert.equal(header.includes("@author"), false);
});

test("compareScriptVersions orders dotted numeric versions", () => {
  assert.equal(compareScriptVersions("1.10.0", "1.2.9"), 1);
  assert.equal(compareScriptVersions("1.2.0", "1.2"), 0);
  assert.equal(compareScriptVersions("0.9", "1.0"), -1);
  assert.equal(compareScriptVersions("", "0.0.1"), -1);
});
