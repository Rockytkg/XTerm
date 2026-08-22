/**
 * 统一的 IPC 错误契约解析。
 *
 * Tauri 命令的失败统一以 `Err(String)` 到达前端：新契约是内嵌 JSON
 * （`{"code": "...", "detail": "...", "retryable": bool}`），旧版本也可能
 * 是裸字符串或已结构化的对象。这里把三种形态归一为
 * `{ title, message, detail, code, recoverable }`，并提供
 * `parseIpcError` 输出更精简的 `{ code, detail, retryable }` 视图。
 */

const CONNECTION_ERROR_FALLBACK = {
  title: "",
  message: "",
  detail: "",
  code: "unknown",
  recoverable: false,
};

function tryParseJson(str) {
  try {
    return JSON.parse(str);
  } catch {
    return null;
  }
}

function errorFromObject(obj) {
  if (!obj || typeof obj !== "object") return null;
  const code = obj.code || obj.errorCode || "";
  const message = obj.message || "";
  const detail = obj.detail || "";
  if (!code && !message && !detail) return null;
  return {
    title: obj.title || "",
    message,
    detail,
    code: code || "unknown",
    recoverable: !!(obj.recoverable ?? obj.retryable),
    args: obj.args && typeof obj.args === "object" ? obj.args : undefined,
  };
}

export function formatConnectionError(error) {
  if (!error) return { ...CONNECTION_ERROR_FALLBACK };
  if (typeof error === "object") {
    return (
      errorFromObject(tryParseJson(error.message)) ??
      errorFromObject(error) ?? {
        ...CONNECTION_ERROR_FALLBACK,
        message: String(error.message || error),
      }
    );
  }
  if (typeof error === "string") {
    return (
      errorFromObject(tryParseJson(error)) ?? {
        ...CONNECTION_ERROR_FALLBACK,
        message: error,
      }
    );
  }
  return { ...CONNECTION_ERROR_FALLBACK, message: String(error) };
}

/**
 * 精简错误契约视图：`{ code, detail, retryable }`。
 * code 统一为小写 snake_case（新契约），未知错误为 "unknown"。
 */
export function parseIpcError(error) {
  const formatted = formatConnectionError(error);
  return {
    code: String(formatted.code || "unknown"),
    detail: String(formatted.detail || formatted.message || ""),
    retryable: formatted.recoverable === true,
    args: formatted.args,
  };
}

/**
 * 后端会话/连接已不在活跃状态——属于预期内的关闭竞态，调用方应静默容错。
 */
export function isConnectionNotActiveError(error) {
  return parseIpcError(error).code === "connection_not_active";
}
