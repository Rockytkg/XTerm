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
