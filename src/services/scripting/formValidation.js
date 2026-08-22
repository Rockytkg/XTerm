// 脚本表单校验：必填（required）、内置格式规则（type: url/email/phone/number）
// 与自定义正则（pattern），供脚本交互弹窗（xterm.form/input）与新建脚本弹窗共用。
// 返回 { fieldKey: 错误码 }，空对象表示全部通过；错误码对应 scripts.validation.* 文案，
// 字段可用 message 自定义错误文案覆盖。

const EMAIL_RE = /^[\w.+-]+@[\w-]+(\.[\w-]+)+$/;
// 中国大陆手机号：1 开头，第二位 3-9，共 11 位。
const PHONE_RE = /^1[3-9]\d{9}$/;

const BUILTIN_RULES = Object.freeze({
  url: { error: "invalidUrl", test: (value) => isValidHttpUrl(value) },
  email: { error: "invalidEmail", test: (value) => EMAIL_RE.test(value) },
  phone: { error: "invalidPhone", test: (value) => PHONE_RE.test(value) },
  number: { error: "invalidNumber", test: (value) => !Number.isNaN(Number(value)) },
});

export function isValidHttpUrl(value) {
  try {
    const url = new URL(String(value).trim());
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function isEmptyValue(value) {
  return value === undefined || value === null || String(value).trim() === "";
}

function toRegExp(pattern) {
  if (pattern instanceof RegExp) return pattern;
  if (typeof pattern === "string" && pattern) {
    try {
      return new RegExp(pattern);
    } catch {
      return null;
    }
  }
  return null;
}

export function validateScriptFields(fields, values) {
  const errors = {};
  for (const field of Array.isArray(fields) ? fields : []) {
    if (!field?.key) continue;
    const value = values?.[field.key];
    if (["switch", "checkbox"].includes(field.type)) continue;

    if (field.required && isEmptyValue(value)) {
      errors[field.key] = "required";
      continue;
    }
    if (isEmptyValue(value)) continue;

    const rule = BUILTIN_RULES[field.type];
    if (rule && !rule.test(String(value).trim())) {
      errors[field.key] = rule.error;
      continue;
    }
    const pattern = toRegExp(field.pattern);
    if (pattern && !pattern.test(String(value))) {
      errors[field.key] = "patternMismatch";
    }
  }
  return errors;
}
