import { createRuntimeId } from "../../utils/runtimeIds.js";
import { invokeDetailedIpc } from "./core";

/**
 * 生成 service 模块共用的请求包装：分配 requestId、绑定模块级 logger
 * 上下文（module/action/自定义 context），并归一 invokeDetailedIpc 的
 * 日志级别选项。各 service 保留自己的命名函数，内部委托给该工厂，
 * 调用方签名与日志行为保持不变。
 */
export function createServiceRunner({ logger, module }) {
  return function runServiceRequest(command, payload, options = {}) {
    const requestId = options.requestId || createRuntimeId();
    const scopedLogger = logger.withContext({
      requestId,
      module,
      action: options.action || command,
      ...(options.context || {}),
    });
    return invokeDetailedIpc(command, payload, {
      requestId,
      scope: scopedLogger,
      level: options.level || "info",
      successLevel: options.successLevel || options.level || "info",
      summarizePayload: options.summarizePayload,
    });
  };
}
