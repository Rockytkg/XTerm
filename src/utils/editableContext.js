// 右键菜单命中判断：事件落在输入类元素上时让位给原生编辑菜单
export function isEditableContextTarget(target) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target?.isContentEditable
  );
}

export function contextMenuItem(id, label, icon, enabled, action, options = {}) {
  return {
    id,
    label,
    icon,
    enabled,
    action,
    ...options,
  };
}
