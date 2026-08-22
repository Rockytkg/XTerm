import { createLogger } from "../../utils/logger.js";
import { invokeDetailedIpc } from "../ipc/core.js";

const fileLogger = createLogger("frontend.scripting.file");

// labels: { title, jsFilesLabel, allFilesLabel } —— 由调用方按当前语言传入。
export function pickScriptFile(labels) {
  return invokeDetailedIpc(
    "script_pick_file",
    { labels },
    {
      scope: fileLogger,
      level: "info",
      successLevel: "info",
    },
  );
}

// 弹原生保存对话框把脚本导出为 .js 文件；用户取消返回 null，成功返回写入路径。
export function exportScriptFile(fileName, code, labels) {
  return invokeDetailedIpc(
    "script_export_file",
    { request: { fileName, code, labels } },
    {
      scope: fileLogger,
      level: "info",
      successLevel: "info",
    },
  );
}

// 拉取 @updateURL 指向的远程脚本内容（油猴式更新检测）。
export function fetchScriptText(url) {
  return invokeDetailedIpc(
    "script_fetch_text",
    { url },
    {
      scope: fileLogger,
      level: "info",
      successLevel: "debug",
    },
  );
}

// 脚本数据读取：原生文件选择器由用户选定文件，返回 { name, content }；取消返回 null。
export function readDataFile(labels) {
  return invokeDetailedIpc(
    "script_read_data_file",
    { labels },
    {
      scope: fileLogger,
      level: "info",
      successLevel: "info",
      summarizeResult: (value) => (value ? { name: value.name } : value),
    },
  );
}

// 脚本数据保存：原生保存对话框写入文本；取消返回 null，成功返回写入路径。
export function writeDataFile(fileName, content, labels) {
  return invokeDetailedIpc(
    "script_write_data_file",
    { request: { fileName, content, labels } },
    {
      scope: fileLogger,
      level: "info",
      successLevel: "info",
      summarizePayload: () => ({ fileName }),
    },
  );
}
