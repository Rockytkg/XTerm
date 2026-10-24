import { Unicode11Addon } from "@xterm/addon-unicode11";

function createUnicode11Provider() {
  let provider = null;
  const addon = new Unicode11Addon();
  // 只捕获 addon 公开注册的 provider，让布局与录制复用 xterm 的 Unicode 11 宽度表。
  addon.activate({
    unicode: {
      register(value) {
        provider = value;
      },
    },
  });
  addon.dispose();
  if (!provider) throw new Error("Unicode 11 provider was not registered");
  return provider;
}

const unicode11Provider = createUnicode11Provider();

export function getTerminalCodePointWidth(codePoint) {
  return unicode11Provider.wcwidth(codePoint);
}

export function getTerminalTextWidth(value) {
  const text = String(value ?? "");
  let width = 0;
  let hasBaseCell = false;
  for (const character of text) {
    const characterWidth = getTerminalCodePointWidth(character.codePointAt(0));
    if (characterWidth === 0 && !hasBaseCell) {
      width += 1;
      continue;
    }
    width += characterWidth;
    if (characterWidth > 0) hasBaseCell = true;
  }
  return width;
}
