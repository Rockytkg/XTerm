const LOG_LEVEL_PRIORITY = Object.freeze({
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
  trace: 4,
});
const DEV_CONSOLE_ENABLED = import.meta.env?.DEV === true;
const DEFAULT_LOG_LEVEL = DEV_CONSOLE_ENABLED ? "debug" : "error";

let activeLogLevel = DEFAULT_LOG_LEVEL;
let logSequence = 0;

function normalizeLogLevel(level) {
  return level in LOG_LEVEL_PRIORITY ? level : DEFAULT_LOG_LEVEL;
}

function shouldLog(level) {
  return DEV_CONSOLE_ENABLED && LOG_LEVEL_PRIORITY[level] <= LOG_LEVEL_PRIORITY[activeLogLevel];
}

// 生产环境把 error/warn 转发到 tauri-plugin-log 的 Webview target，写入后端日志文件；
// info/debug 仍只在 DEV 输出。动态加载避免 node 单测与非 Tauri 环境解析失败。
let pluginLogModulePromise = null;

function pluginLogModule() {
  if (!pluginLogModulePromise) {
    pluginLogModulePromise = import("@tauri-apps/plugin-log").catch(() => null);
  }
  return pluginLogModulePromise;
}

function shouldForwardToBackend(level) {
  if (DEV_CONSOLE_ENABLED) return false;
  if (level !== "error" && level !== "warn") return false;
  if (typeof window === "undefined") return false;
  return LOG_LEVEL_PRIORITY[level] <= LOG_LEVEL_PRIORITY[activeLogLevel];
}

// 单条日志 emit 的统一门控：console 输出或后端转发任一通即可。
// isLogLevelEnabled 与 ScopedLogger.emit 共用；级别关闭时调用方可跳过
// 组装日志实参（如 summarizeValue 序列化），输出结果不变。
function isEmitEnabled(level) {
  return shouldLog(level) || shouldForwardToBackend(level);
}

export function isLogLevelEnabled(level) {
  return isEmitEnabled(level);
}

function stringifyDetails(details) {
  try {
    return JSON.stringify(details);
  } catch {
    return "[unserializable details]";
  }
}

function forwardToBackendLog(level, scope, message, entry) {
  void pluginLogModule().then((mod) => {
    const forward = level === "error" ? mod?.error : mod?.warn;
    if (typeof forward !== "function") return;
    const details = entry.details?.length ? ` ${stringifyDetails(entry.details)}` : "";
    void forward(`[${scope}] ${message}${details}`).catch(() => {});
  });
}

function padTimestampPart(value) {
  return String(value).padStart(2, "0");
}

function formatLocalTimestamp(date = new Date()) {
  const year = date.getFullYear();
  const month = padTimestampPart(date.getMonth() + 1);
  const day = padTimestampPart(date.getDate());
  const hours = padTimestampPart(date.getHours());
  const minutes = padTimestampPart(date.getMinutes());
  const seconds = padTimestampPart(date.getSeconds());

  return {
    date: `${year}-${month}-${day}`,
    time: `${hours}:${minutes}:${seconds}`,
  };
}

function summarizeString(value) {
  if (value.length <= 160) return value;
  return `${value.slice(0, 157)}...`;
}

function summarizeError(error) {
  return {
    name: error?.name || "Error",
    message: error?.message || String(error),
    code: error?.code,
    detail: error?.detail,
  };
}

function summarizeObject(value, depth, seen) {
  if (!value || typeof value !== "object") return value;
  if (seen.has(value)) return "[Circular]";
  seen.add(value);

  if (Array.isArray(value)) {
    const items = value.slice(0, 5).map((item) => summarizeValue(item, depth + 1, seen));
    if (value.length > items.length) items.push(`...(${value.length - items.length} more)`);
    return items;
  }

  const entries = Object.entries(value);
  const summary = {};
  for (const [index, [key, entryValue]] of entries.entries()) {
    if (index >= 8) {
      summary.__truncated = `${entries.length - index} more keys`;
      break;
    }
    summary[key] = summarizeValue(entryValue, depth + 1, seen);
  }
  return summary;
}

export function summarizeValue(value, depth = 0, seen) {
  if (value instanceof Error) return summarizeError(value);
  if (value === null || value === undefined) return value;
  if (typeof value === "string") return summarizeString(value);
  if (typeof value === "number" || typeof value === "boolean") return value;
  if (typeof value === "bigint") return value.toString();
  if (typeof value === "function") return `[Function ${value.name || "anonymous"}]`;
  if (depth >= 2) {
    if (Array.isArray(value)) return `[Array(${value.length})]`;
    return "[Object]";
  }
  // Lazy WeakSet — only create when we actually need cycle detection
  const cycleGuard = seen || new WeakSet();
  return summarizeObject(value, depth, cycleGuard);
}

function makeEntry(scope, level, context, args) {
  const timestamp = formatLocalTimestamp();
  const [firstArg, ...restArgs] = args;
  const hasStringMessage = typeof firstArg === "string";
  const message = hasStringMessage ? firstArg : "event";
  const details = (hasStringMessage ? restArgs : args).map((value) => summarizeValue(value));

  const entry = {
    seq: ++logSequence,
    date: timestamp.date,
    time: timestamp.time,
    level,
    scope,
    ...context,
  };

  if (details.length > 0) {
    entry.details = details;
  }

  return { entry, message };
}

function consoleMethod(level) {
  if (level === "trace") return console.debug;
  return console[level] || console.log;
}

class ScopedLogger {
  constructor(scope, context = {}) {
    this.scope = scope;
    this.context = context;
  }

  child(scopeOrContext, context = {}) {
    if (typeof scopeOrContext === "string") {
      return new ScopedLogger(`${this.scope}.${scopeOrContext}`, {
        ...this.context,
        ...context,
      });
    }
    return new ScopedLogger(this.scope, {
      ...this.context,
      ...(scopeOrContext || {}),
    });
  }

  withContext(context = {}) {
    return new ScopedLogger(this.scope, { ...this.context, ...context });
  }

  emit(level, ...args) {
    const consoleEnabled = shouldLog(level);
    if (!consoleEnabled && !isEmitEnabled(level)) return;
    const { entry, message } = makeEntry(this.scope, level, this.context, args);
    if (!consoleEnabled) {
      forwardToBackendLog(level, this.scope, message, entry);
      return;
    }
    const prefix = `[${entry.date}][${entry.time}][${level.toUpperCase()}] [${this.scope}]`;
    consoleMethod(level)(`${prefix} ${message}`, entry);
  }

  error(...args) {
    this.emit("error", ...args);
  }
  warn(...args) {
    this.emit("warn", ...args);
  }
  info(...args) {
    this.emit("info", ...args);
  }
  debug(...args) {
    this.emit("debug", ...args);
  }
  trace(...args) {
    this.emit("trace", ...args);
  }
}

export function createLogger(scope, context = {}) {
  return new ScopedLogger(scope, context);
}

export function setGlobalLogLevel(level) {
  activeLogLevel = normalizeLogLevel(level);
  return activeLogLevel;
}

export function getGlobalLogLevel() {
  return activeLogLevel;
}
