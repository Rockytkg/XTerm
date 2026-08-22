import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { serializeSanitizedNode } from "../src/utils/sanitizeHtml.js";

// sanitizeHtml 依赖浏览器 DOMParser（node 测试环境没有），
// 这里用极简伪节点直接驱动安全核心 serializeSanitizedNode。
const el = (tag, children = [], attributes = []) => ({
  nodeType: 1,
  tagName: tag.toUpperCase(),
  attributes,
  childNodes: children,
});
const text = (value) => ({ nodeType: 3, nodeValue: value });
const comment = (value = "") => ({ nodeType: 8, nodeValue: value });
const attr = (name, value) => ({ name, value });
const render = (...nodes) => nodes.map(serializeSanitizedNode).join("");

describe("serializeSanitizedNode", () => {
  it("转义文本节点", () => {
    assert.equal(
      render(text('<img src=x onerror="alert(1)"> & "x"')),
      "&lt;img src=x onerror=&quot;alert(1)&quot;&gt; &amp; &quot;x&quot;",
    );
  });

  it("script/style 等危险标签连同内容整块丢弃", () => {
    for (const tag of [
      "script",
      "style",
      "iframe",
      "object",
      "embed",
      "template",
      "form",
      "input",
      "button",
      "link",
      "meta",
      "base",
    ]) {
      assert.equal(render(el(tag, [text("evil")])), "", tag);
    }
  });

  it("未知标签解包保留子节点", () => {
    assert.equal(render(el("marquee", [text("hi"), el("b", [text("!")])])), "hi<b>!</b>");
  });

  it("注释节点丢弃", () => {
    assert.equal(render(comment("x")), "");
  });

  it("允许标签原样保留，属性一律剥除", () => {
    assert.equal(
      render(el("div", [text("x")], [attr("onclick", "alert(1)"), attr("class", "c")])),
      "<div>x</div>",
    );
  });

  it("br/hr 为 void 标签", () => {
    assert.equal(
      render(el("p", [text("a"), el("br"), text("b"), el("hr", [text("ignored")])])),
      "<p>a<br>b<hr></p>",
    );
  });

  it("a 仅保留安全的 http(s)/mailto href，并强制 target/rel", () => {
    assert.equal(
      render(el("a", [text("ok")], [attr("href", "https://example.com"), attr("onclick", "x")])),
      '<a href="https://example.com" target="_blank" rel="noopener noreferrer">ok</a>',
    );
    assert.equal(
      render(el("a", [text("mail")], [attr("href", "mailto:a@b.c")])),
      '<a href="mailto:a@b.c" target="_blank" rel="noopener noreferrer">mail</a>',
    );
  });

  it("危险协议 href 剥除但保留文字", () => {
    for (const href of [
      "javascript:alert(1)",
      "JaVaScRiPt:alert(1)",
      "data:text/html,<script>",
      "  javascript:x",
      "jav\tascript:x",
      "#frag",
      "",
    ]) {
      assert.equal(render(el("a", [text("t")], [attr("href", href)])), "<a>t</a>", href);
    }
  });

  it("href 中的引号被转义，无法闭合属性", () => {
    assert.equal(
      render(el("a", [text("t")], [attr("href", 'https://a.b/" onclick="x')])),
      '<a href="https://a.b/&quot; onclick=&quot;x" target="_blank" rel="noopener noreferrer">t</a>',
    );
  });

  it("嵌套表格结构保留", () => {
    const table = el("table", [
      el("thead", [el("tr", [el("th", [text("h")])])]),
      el("tbody", [el("tr", [el("td", [text("v")])])]),
    ]);
    assert.equal(
      render(table),
      "<table><thead><tr><th>h</th></tr></thead><tbody><tr><td>v</td></tr></tbody></table>",
    );
  });

  it("非元素/文本节点一律忽略", () => {
    assert.equal(render({ nodeType: 11, childNodes: [text("x")] }), "");
  });
});
