import { createLogger } from "../../utils/logger.js";

const bridgeLogger = createLogger("frontend.scripting.bridge");

// 脚本引擎与宿主能力之间的桥注册表：
// - 终端桥：TerminalPanel 为每个终端加载 ScriptBridgeAddon 后按前端会话 id 注册；
//   输出数据经 publishTerminalOutput 喂给对应 addon 的监听器（与渲染同源）。
// - 记录桥：workspaceRecordingController 注册全局适配器，脚本按会话 id 启停记录。
// runner 不直接 import Pinia store / addon 类，保持 node 单测可加载。
const scriptBridges = new Map();
let recordingBridge = null;

export function registerScriptBridge(frontendSessionId, bridge) {
  if (!frontendSessionId || !bridge) return () => {};
  scriptBridges.set(frontendSessionId, bridge);
  return () => {
    if (scriptBridges.get(frontendSessionId) === bridge) {
      scriptBridges.delete(frontendSessionId);
    }
  };
}

export function getScriptBridge(frontendSessionId) {
  return frontendSessionId ? scriptBridges.get(frontendSessionId) || null : null;
}

export function publishTerminalOutput(frontendSessionId, data) {
  if (!frontendSessionId || !data) return;
  try {
    getScriptBridge(frontendSessionId)?.notifyOutput(data);
  } catch (error) {
    bridgeLogger.warn("script bridge output listener failed:", error);
  }
}

export function registerRecordingBridge(adapter) {
  if (!adapter) return () => {};
  recordingBridge = adapter;
  return () => {
    if (recordingBridge === adapter) recordingBridge = null;
  };
}

export function getRecordingBridge() {
  return recordingBridge;
}
