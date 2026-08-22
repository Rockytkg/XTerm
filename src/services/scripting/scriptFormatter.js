import * as prettier from "prettier/standalone";
import babel from "prettier/plugins/babel";
import estree from "prettier/plugins/estree";

const plugins = [babel, estree];

function formatterOptions(options = {}) {
  return {
    parser: "babel",
    plugins,
    semi: true,
    singleQuote: false,
    tabWidth: options.tabWidth ?? 2,
    printWidth: 100,
    trailingComma: "all",
  };
}

export function formatScript(code, options = {}) {
  return prettier.format(String(code), formatterOptions(options));
}

export function formatScriptWithCursor(code, cursorOffset, options = {}) {
  return prettier.formatWithCursor(String(code), {
    ...formatterOptions(options),
    cursorOffset,
  });
}
