export const CONTEXT_MENU_LAYOUT = Object.freeze({
  itemHeight: 32,
  maxHeight: 540,
  minHeight: 44,
  screenMargin: 8,
  separatorHeight: 9,
  verticalPadding: 12,
  width: 179,
});

/** 危险（破坏性）菜单项的统一判定。 */
export function isContextMenuDangerItem(item) {
  return !!(item?.tone === "danger" || item?.id?.includes("delete"));
}

/** 可激活项：普通条目且未禁用（分隔符与禁用项不参与高亮/键盘导航）。 */
export function isContextMenuActivatable(item) {
  return !!item && item.type !== "separator" && item.enabled !== false;
}

/** 禁用但仍保留展示的全局编辑项：原生菜单里剪切/复制/粘贴以灰色呈现而不是消失。 */
const PRESERVED_DISABLED_ITEM_IDS = new Set(["global-cut", "global-copy", "global-paste"]);

/**
 * 菜单项规整：拍平嵌套数组、丢弃空项与未保留的禁用项、
 * 合并相邻分隔符并去掉首尾分隔符，为缺 id 的项补 id。
 */
export function normalizeContextMenuItems(rawItems) {
  const normalized = [];
  for (const item of rawItems.flat().filter(Boolean)) {
    if (item.type === "separator") {
      if (normalized.length && normalized[normalized.length - 1].type !== "separator") {
        normalized.push({ id: `separator-${normalized.length}`, type: "separator" });
      }
      continue;
    }

    const normalizedItem = {
      enabled: item.enabled !== false,
      ...item,
      id: item.id || `item-${normalized.length}`,
      type: "item",
    };
    if (normalizedItem.enabled === false && !PRESERVED_DISABLED_ITEM_IDS.has(normalizedItem.id)) {
      continue;
    }
    normalized.push(normalizedItem);
  }

  while (normalized[normalized.length - 1]?.type === "separator") normalized.pop();
  return normalized;
}

/** 菜单高度：按条目/分隔符高度累加，夹在最小/最大高度之间。 */
export function contextMenuHeight(items) {
  const contentHeight = items.reduce(
    (height, item) =>
      height +
      (item.type === "separator"
        ? CONTEXT_MENU_LAYOUT.separatorHeight
        : CONTEXT_MENU_LAYOUT.itemHeight),
    CONTEXT_MENU_LAYOUT.verticalPadding,
  );
  return Math.min(
    CONTEXT_MENU_LAYOUT.maxHeight,
    Math.max(CONTEXT_MENU_LAYOUT.minHeight, contentHeight),
  );
}

/**
 * 菜单定位：优先落在指针右下方；视口边缘空间不足时翻到指针另一侧展开
 * （与原生菜单一致），最后收拢进可视区安全边距内。
 */
export function contextMenuPosition({ x, y, width, height, viewWidth, viewHeight }) {
  const margin = CONTEXT_MENU_LAYOUT.screenMargin;
  const place = (pointer, size, viewSize) => {
    const maxStart = Math.max(margin, viewSize - margin - size);
    const start = pointer + size + margin <= viewSize ? pointer : pointer - size;
    return Math.min(Math.max(start, margin), maxStart);
  };
  return { x: place(x, width, viewWidth), y: place(y, height, viewHeight) };
}

/**
 * 从 fromIndex（不含）起沿 direction 找下一个可激活项，跳过禁用项与
 * 分隔符，到末尾环绕（Home/End 用 fromIndex=-1/items.length 实现）。
 * 没有可激活项时返回 -1。
 */
export function nextActivatableMenuIndex(items, fromIndex, direction = 1) {
  const count = items?.length || 0;
  for (let step = 1; step <= count; step += 1) {
    const index = (((fromIndex + step * direction) % count) + count) % count;
    if (isContextMenuActivatable(items[index])) return index;
  }
  return -1;
}

/**
 * 首字符定位（typeahead）：跳到下一个标签以 char 开头的可激活项，
 * 环绕、大小写不敏感；与原生菜单的按键搜索一致。找不到返回 -1。
 */
export function typeaheadMenuIndex(items, fromIndex, char, textOf = (item) => item?.label) {
  const query = String(char || "")
    .trim()
    .toLowerCase();
  const count = items?.length || 0;
  if (!query || !count) return -1;
  for (let step = 1; step <= count; step += 1) {
    const index = (fromIndex + step) % count;
    const item = items[index];
    if (!isContextMenuActivatable(item)) continue;
    if (
      String(textOf(item) || "")
        .trim()
        .toLowerCase()
        .startsWith(query)
    )
      return index;
  }
  return -1;
}
