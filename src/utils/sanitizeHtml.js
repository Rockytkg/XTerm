// 脚本弹窗自定义 HTML 的消毒器：DOMParser 解析后经严格白名单重序列化输出，
// 绝不原样插入输入字符串，因此不存在漏转义的注入面。
// 核心 walker 只依赖 nodeType/tagName/attributes/childNodes/nodeValue，
// 方便在 Node 测试里用极简伪节点驱动。

const ELEMENT_NODE = 1;
const TEXT_NODE = 3;

// 允许保留的标签；其余标签解包（保留子节点）。
const ALLOWED_TAGS = new Set([
  "a",
  "b",
  "strong",
  "i",
  "em",
  "u",
  "s",
  "code",
  "pre",
  "br",
  "p",
  "span",
  "div",
  "ul",
  "ol",
  "li",
  "blockquote",
  "h1",
  "h2",
  "h3",
  "h4",
  "hr",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
]);

// 这些标签连同内容整块丢弃（脚本、样式、嵌入内容、表单控件）。
const DROP_WITH_CONTENT = new Set([
  "script",
  "style",
  "iframe",
  "object",
  "embed",
  "template",
  "noscript",
  "form",
  "input",
  "button",
  "textarea",
  "select",
  "link",
  "meta",
  "base",
]);

const VOID_TAGS = new Set(["br", "hr"]);
const SAFE_HREF = /^(?:https?:|mailto:)/i;

function escapeHtml(text) {
  return String(text)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

// 链接只允许 http(s)/mailto，且强制新窗口 + noopener；
// 其余协议（javascript:、data: 等）剥掉 href，保留文字。
function safeAttributes(tag, attributes) {
  if (tag !== "a") return "";
  for (const attr of attributes || []) {
    if (attr.name?.toLowerCase() !== "href") continue;
    const href = String(attr.value || "").trim();
    if (SAFE_HREF.test(href)) {
      return ` href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer"`;
    }
  }
  return "";
}

function serializeChildren(node) {
  let out = "";
  for (const child of node.childNodes || []) out += serializeSanitizedNode(child);
  return out;
}

export function serializeSanitizedNode(node) {
  if (node.nodeType === TEXT_NODE) return escapeHtml(node.nodeValue || "");
  if (node.nodeType !== ELEMENT_NODE) return ""; // 注释等一律丢弃

  const tag = String(node.tagName || "").toLowerCase();
  if (DROP_WITH_CONTENT.has(tag)) return "";
  if (!ALLOWED_TAGS.has(tag)) return serializeChildren(node); // 解包未知标签

  if (VOID_TAGS.has(tag)) return `<${tag}>`;
  return `<${tag}${safeAttributes(tag, node.attributes)}>${serializeChildren(node)}</${tag}>`;
}

// 把脚本提供的 HTML 消毒为可安全 v-html 渲染的字符串。
export function sanitizeHtml(html) {
  const doc = new DOMParser().parseFromString(String(html ?? ""), "text/html");
  return serializeChildren(doc.body);
}
