// 状态栏快捷按钮的纯数据模型：normalize 不依赖 Vue/IPC，便于 node 单测。
export const DEFAULT_COLOR = "#4f8cff";

export function normalizeQuickButton(raw) {
  if (!raw || !raw.id || !String(raw.name || "").trim()) return null;
  return {
    id: String(raw.id),
    name: String(raw.name).trim(),
    type: raw.type === "script" ? "script" : "send",
    value: String(raw.value ?? ""),
    color: String(raw.color || DEFAULT_COLOR),
  };
}
