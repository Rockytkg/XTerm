// 把用户输入的转义序列（\n、\x1b、\u{1f600} 等）展开为真实控制字符，
// 供状态栏快捷按钮等"发送原始内容"的入口使用。未识别的反斜杠序列保持原样。
const ESCAPE_PATTERN = /\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|n|r|t|b|f|v|0|e|\\)/g;

// 发送内容中的暂停标记：\d<毫秒>，如 "enable\n\d300\nconfig" 表示发送 enable 后停 300ms。
// 解析必须在 expandTerminalEscapes 之前对原文进行，否则 \\d500 这类"字面 \d500"
// 会被误当作暂停；切分后的文本段仍含未展开的转义，由调用方逐段展开。
const DELAY_PATTERN = /^\\d(\d+)/;

export function expandTerminalEscapes(value) {
  return String(value ?? "").replace(ESCAPE_PATTERN, (match, token) => {
    if (token === "n") return "\n";
    if (token === "r") return "\r";
    if (token === "t") return "\t";
    if (token === "b") return "\b";
    if (token === "f") return "\f";
    if (token === "v") return "\v";
    if (token === "0") return "\0";
    if (token === "e") return "\x1b";
    if (token === "\\") return "\\";
    const hex = token.startsWith("u{") ? token.slice(2, -1) : token.slice(1);
    return String.fromCodePoint(parseInt(hex, 16));
  });
}

// 把发送内容切成 text / delay 段序列，供调用方按序"发送文本、等待毫秒"。
// 反斜杠成对出现（\\）时整体留给文本段，保证 \\d500 展开后仍是字面 \d500。
export function splitTerminalSendContent(value) {
  const source = String(value ?? "");
  const segments = [];
  let text = "";
  let i = 0;
  while (i < source.length) {
    if (source[i] === "\\") {
      if (source[i + 1] === "\\") {
        text += "\\\\";
        i += 2;
        continue;
      }
      const delay = DELAY_PATTERN.exec(source.slice(i));
      if (delay) {
        if (text) {
          segments.push({ type: "text", text });
          text = "";
        }
        segments.push({ type: "delay", ms: Number(delay[1]) });
        i += delay[0].length;
        continue;
      }
    }
    text += source[i];
    i += 1;
  }
  if (text) segments.push({ type: "text", text });
  return segments;
}
