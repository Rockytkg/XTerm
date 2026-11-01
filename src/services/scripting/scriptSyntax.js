let formatterPromise = null;

// 沙盒屏蔽清单：这些全局名会注入"访问即抛错"的占位值，把网络、宿主桥
// （Tauri IPC）、DOM、本地存储、代码执行等能力从脚本作用域
// 中摘除；脚本只能用 xterm.* 专用函数申请这些能力（会弹窗请求用户授权）。
// 注意这是同 JS realm 内的尽力隔离：决心绕过的脚本仍可能经构造器原型链
// （如 ({}).constructor.constructor）拿到真全局；真正的兜底是 CSP
// （connect-src 禁止外联）与"敏感能力一律授权弹窗"的设计，第三方脚本仍需审查。
export const SANDBOX_BLOCKED_GLOBALS = Object.freeze([
  // 网络与外部通信
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "EventSource",
  "WebTransport",
  "BroadcastChannel",
  "Worker",
  "SharedWorker",
  "importScripts",
  // 宿主桥：可直接调用任意 Tauri 命令（withGlobalTauri 注入），必须封死
  "__TAURI__",
  "__TAURI_INTERNALS__",
  "__TAURI_METADATA__",
  // 全局对象与 DOM
  "window",
  "self",
  "globalThis",
  "document",
  "top",
  "parent",
  "frames",
  "opener",
  "location",
  "history",
  "navigator",
  "screen",
  "external",
  "clientInformation",
  "visualViewport",
  // 本地持久化存储
  "localStorage",
  "sessionStorage",
  "indexedDB",
  "caches",
  "openDatabase",
  // 代码执行与模块加载（脚本本体由引擎编译，无需二次动态代码生成。
  // eval 不能作为 strict 模式下的形参名，无法遮蔽：直接 eval 仍在沙盒作用域内
  // 解析全局名（依旧命中下面的屏蔽值），间接 eval (0,eval) 属于已知残留逃逸路径，
  // 与构造器原型链逃逸同级，由 CSP 与使用规范兜底。）
  "Function",
  "require",
  "module",
  "exports",
  "process",
  "global",
  // 原生弹窗与系统交互（脚本应使用 xterm.input/confirm/alert）
  "alert",
  "confirm",
  "prompt",
  "print",
  "open",
  "close",
  "stop",
  "postMessage",
  "Notification",
  "SharedArrayBuffer",
]);

function defaultBlockedMessage(name) {
  return `"${name}" is blocked by the script sandbox; use the dedicated xterm.* API (it asks for user authorization) instead`;
}

// 生成某个被禁全局的占位值：任何调用、new、属性读写都抛出点名该 API 的错误；
// 只放行 Symbol.toPrimitive，让日志序列化得到可读标记而不是二次抛错。
function createBlockedValue(name, formatMessage) {
  const raise = () => {
    throw new Error(formatMessage(name));
  };
  return new Proxy(function blockedSandboxGlobal() {}, {
    apply: raise,
    construct: raise,
    set: raise,
    get(_, property) {
      if (property === Symbol.toPrimitive) return () => `[sandbox blocked: ${name}]`;
      raise();
    },
  });
}

// 按 SANDBOX_BLOCKED_GLOBALS 的顺序生成对应的屏蔽值（每次运行新建，避免跨运行共享状态）。
export function createSandboxBindings(formatMessage = defaultBlockedMessage) {
  return SANDBOX_BLOCKED_GLOBALS.map((name) => createBlockedValue(name, formatMessage));
}

function createScope(scopeNames, values) {
  const scope = Object.create(null);
  for (let index = 0; index < scopeNames.length; index += 1) {
    scope[scopeNames[index]] = values[index];
  }
  // `with` consults `has` for every free identifier. Returning true only for
  // names injected into this scope keeps unrelated globals on the normal
  // sandboxed global path.
  return new Proxy(scope, {
    has(target, property) {
      return typeof property === "string" && Object.prototype.hasOwnProperty.call(target, property);
    },
  });
}

export function compileScript(code = "", scopeNames = []) {
  const source = String(code);
  // The generated outer function is intentionally non-strict so it can create
  // a lexical `with` environment.  User code itself remains strict inside the
  // async function, preserving predictable `this`, octal and assignment rules.
  // Unlike parameter injection, this permits normal top-level declarations such
  // as `const console = ...` without colliding with runtime bindings.
  const createScript = new Function(
    "__scope",
    `with (__scope) { return async function __xterm_script__(${SANDBOX_BLOCKED_GLOBALS.join(",")}) { "use strict";\n${source}\n}; }`,
  );

  return (xterm, ...values) => {
    const runtimeValueCount = scopeNames.length;
    const runtimeValues = [xterm, ...values.slice(0, runtimeValueCount)];
    const blockedValues = values.slice(runtimeValueCount);
    return createScript(createScope(["xterm", ...scopeNames], runtimeValues))(...blockedValues);
  };
}

/** Compile a script with the same async function shape used by the runner. */
export function validateScriptSyntax(code = "", scopeNames = []) {
  try {
    compileScript(code, scopeNames);
    return null;
  } catch (error) {
    return error instanceof Error ? error : new Error(String(error));
  }
}

function loadFormatter() {
  formatterPromise ??= import("./scriptFormatter.js").catch((error) => {
    // 开发服务器依赖更新可能让懒加载请求短暂失效；清除缓存后允许用户重试。
    formatterPromise = null;
    throw error;
  });
  return formatterPromise;
}

/** Format a script on demand so the formatter does not inflate the initial editor chunk. */
export async function formatScript(code, options = {}) {
  const formatter = await loadFormatter();
  return formatter.formatScript(code, options);
}

export async function formatScriptWithCursor(code, cursorOffset, options = {}) {
  const formatter = await loadFormatter();
  return formatter.formatScriptWithCursor(code, cursorOffset, options);
}
