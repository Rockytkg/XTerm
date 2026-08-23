import { reactive } from "vue";
import {
  readText as readClipboardText,
  writeText as writeClipboardText,
} from "@tauri-apps/plugin-clipboard-manager";
import { i18n } from "../i18n";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { addDomListener } from "../utils/domListeners";
import { CONTEXT_MENU_LAYOUT } from "../utils/contextMenu";
import { isMacPlatform } from "../utils/platform";

const PRESERVE_DISABLED_IDS = new Set(["global-cut", "global-copy", "global-paste"]);
const {
  itemHeight: MENU_ITEM_HEIGHT,
  maxHeight: MENU_MAX_HEIGHT,
  minHeight: MENU_MIN_HEIGHT,
  screenMargin: MENU_SCREEN_MARGIN,
  separatorHeight: MENU_SEPARATOR_HEIGHT,
  verticalPadding: MENU_VERTICAL_PADDING,
  width: MENU_WIDTH,
} = CONTEXT_MENU_LAYOUT;

let initialized = false;
let activeMenuActions = new Map();
const asyncListeners = createAsyncListenerRegistry();

/**
 * 菜单的渲染状态。菜单始终在主窗口内以 DOM 渲染（components/ContextMenu.vue），
 * 用 contextmenu 事件的 clientX/Y（窗口内坐标）定位：Wayland 不允许
 * 客户端查询全局光标位置也不允许程序设置窗口绝对位置，窗口内坐标在
 * 所有平台上都可靠。
 * 只存纯展示数据；动作回调留在非响应式的 activeMenuActions 里。
 */
export const contextMenuState = reactive({
  visible: false,
  items: [],
  x: 0,
  y: 0,
  theme: "light",
  width: MENU_WIDTH,
  maxHeight: MENU_MAX_HEIGHT,
});

function t(key, params) {
  return i18n.global.t(key, params);
}

const INERT_INPUT_TYPES = new Set([
  "button",
  "checkbox",
  "color",
  "file",
  "hidden",
  "image",
  "radio",
  "range",
  "reset",
  "submit",
]);

function isEditableInput(input) {
  return !input.disabled && !input.readOnly && !INERT_INPUT_TYPES.has(input.type);
}

function editableTargetFrom(target) {
  if (!(target instanceof Element)) return null;
  const candidate = target.closest("input, textarea, [contenteditable]");
  if (candidate instanceof HTMLTextAreaElement) {
    return candidate.disabled || candidate.readOnly ? null : candidate;
  }
  if (candidate instanceof HTMLInputElement) {
    return isEditableInput(candidate) ? candidate : null;
  }
  if (candidate?.isContentEditable) return candidate;
  return null;
}

function selectedTextFromEditable(target) {
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = target.selectionStart ?? 0;
    const end = target.selectionEnd ?? start;
    return target.value.slice(start, end);
  }
  return String(window.getSelection?.()?.toString() || "");
}

function selectedTextFromDocument() {
  return String(window.getSelection?.()?.toString() || "");
}

function captureEditableSelection(target) {
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return {
      selectionStart: target.selectionStart ?? target.value.length,
      selectionEnd: target.selectionEnd ?? target.value.length,
    };
  }

  const selection = window.getSelection?.();
  if (!selection || !selection.rangeCount) return {};
  return { range: selection.getRangeAt(0).cloneRange() };
}

function buildEditContext(nativeEvent) {
  const editableTarget = editableTargetFrom(nativeEvent?.target);
  const selectedText = editableTarget
    ? selectedTextFromEditable(editableTarget)
    : selectedTextFromDocument();

  return {
    editableTarget,
    nativeEvent,
    selection: editableTarget ? captureEditableSelection(editableTarget) : {},
    selectedText,
  };
}

function restoreContentEditableRange(target, context) {
  const range = context.selection?.range;
  if (!range) return;
  const selection = window.getSelection?.();
  if (!selection) return;
  target.focus();
  selection.removeAllRanges();
  selection.addRange(range);
}

function insertTextIntoTarget(target, text, context) {
  if (!target) return;

  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = context.selection?.selectionStart ?? target.selectionStart ?? target.value.length;
    const end = context.selection?.selectionEnd ?? target.selectionEnd ?? start;
    target.focus();
    target.setRangeText(text, start, end, "end");
    target.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }

  if (target.isContentEditable) {
    restoreContentEditableRange(target, context);
    const selection = window.getSelection?.();
    if (!selection || !selection.rangeCount) return;
    const range = selection.getRangeAt(0);
    range.deleteContents();
    range.insertNode(document.createTextNode(text));
    range.collapse(false);
    selection.removeAllRanges();
    selection.addRange(range);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

async function copySelection(context) {
  await writeClipboardText(context.selectedText);
}

async function cutSelection(context) {
  if (!context.editableTarget || !context.selectedText) return;
  await writeClipboardText(context.selectedText);
  insertTextIntoTarget(context.editableTarget, "", context);
}

async function pasteIntoTarget(context) {
  if (!context.editableTarget) return;
  const text = await readClipboardText();
  if (!text) return;
  insertTextIntoTarget(context.editableTarget, text, context);
}

function deleteSelection(context) {
  if (!context.editableTarget || !context.selectedText) return;
  insertTextIntoTarget(context.editableTarget, "", context);
}

function selectAll(context) {
  const target = context.editableTarget;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    target.focus();
    target.select();
    return;
  }
  if (target?.isContentEditable) {
    target.focus();
    const range = document.createRange();
    range.selectNodeContents(target);
    const selection = window.getSelection?.();
    selection?.removeAllRanges();
    selection?.addRange(range);
    return;
  }
  const selection = window.getSelection?.();
  if (!selection || !document.body) return;
  const range = document.createRange();
  range.selectNodeContents(document.body);
  selection.removeAllRanges();
  selection.addRange(range);
}

function defaultEditItems(context) {
  const hasSelection = !!context.selectedText;
  const canEdit = !!context.editableTarget;
  // macOS 上编辑快捷键是 Cmd 系，菜单文案同步显示 ⌘。
  const shortcutPrefix = isMacPlatform() ? "⌘" : "Ctrl+";

  return [
    {
      id: "global-cut",
      labelKey: "contextMenu.cut",
      label: t("contextMenu.cut"),
      icon: "cut",
      enabled: canEdit && hasSelection,
      shortcut: `${shortcutPrefix}X`,
      action: () => cutSelection(context),
    },
    {
      id: "global-copy",
      labelKey: "contextMenu.copy",
      label: t("contextMenu.copy"),
      icon: "copy",
      enabled: hasSelection,
      shortcut: `${shortcutPrefix}C`,
      action: () => copySelection(context),
    },
    {
      id: "global-paste",
      labelKey: "contextMenu.paste",
      label: t("contextMenu.paste"),
      icon: "paste",
      enabled: canEdit,
      shortcut: `${shortcutPrefix}V`,
      action: () => pasteIntoTarget(context),
    },
    {
      id: "global-delete",
      labelKey: "contextMenu.delete",
      label: t("contextMenu.delete"),
      icon: "delete",
      enabled: canEdit && hasSelection,
      action: () => deleteSelection(context),
    },
    { type: "separator" },
    {
      id: "global-select-all",
      labelKey: "contextMenu.selectAll",
      label: t("contextMenu.selectAll"),
      icon: "selectAll",
      enabled: canEdit || !!document.body?.innerText,
      shortcut: `${shortcutPrefix}A`,
      action: () => selectAll(context),
    },
  ];
}

function normalizeItems(rawItems) {
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
    if (normalizedItem.enabled === false && !PRESERVE_DISABLED_IDS.has(normalizedItem.id)) {
      continue;
    }
    normalized.push(normalizedItem);
  }

  while (normalized[normalized.length - 1]?.type === "separator") normalized.pop();
  return normalized;
}

function clamp(value, min, max) {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

function finiteNumber(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function menuHeight(items, maxHeight = MENU_MAX_HEIGHT) {
  const contentHeight = items.reduce(
    (height, item) =>
      height + (item.type === "separator" ? MENU_SEPARATOR_HEIGHT : MENU_ITEM_HEIGHT),
    MENU_VERTICAL_PADDING,
  );
  return Math.min(maxHeight, Math.max(MENU_MIN_HEIGHT, contentHeight));
}

/** 菜单项的纯展示视图（剥离动作回调），回调只存在 activeMenuActions 里。 */
function menuItemView(item) {
  if (item.type === "separator") return { id: item.id, type: "separator" };
  return {
    id: item.id,
    type: "item",
    labelKey: item.labelKey || "",
    label: item.label,
    icon: item.icon || "",
    shortcut: item.shortcut || "",
    tone: item.tone || "",
    enabled: item.enabled !== false,
  };
}

function menuActionMap(items, context) {
  return new Map(
    items
      .filter((item) => item.type !== "separator")
      .map((item) => [
        item.id,
        {
          action: item.action,
          context,
          enabled: item.enabled !== false,
        },
      ]),
  );
}

function dismissMenu() {
  activeMenuActions = new Map();
  if (!contextMenuState.visible) return;
  contextMenuState.visible = false;
  contextMenuState.items = [];
}

/**
 * 打开菜单。定位用 contextmenu 事件的 clientX/Y（窗口内坐标），并按
 * 视口边缘收拢。
 */
function openMenu(items, context, nativeEvent) {
  dismissMenu();
  activeMenuActions = menuActionMap(items, context);
  const width = MENU_WIDTH;
  const height = menuHeight(items);
  const viewWidth = Math.max(window.innerWidth || 0, width + MENU_SCREEN_MARGIN * 2);
  const viewHeight = Math.max(window.innerHeight || 0, height + MENU_SCREEN_MARGIN * 2);
  const rawX = finiteNumber(nativeEvent?.clientX, (viewWidth - width) / 2);
  const rawY = finiteNumber(nativeEvent?.clientY, (viewHeight - height) / 2);
  contextMenuState.items = items.map(menuItemView);
  contextMenuState.theme = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  contextMenuState.width = width;
  contextMenuState.maxHeight = height;
  contextMenuState.x = clamp(rawX, MENU_SCREEN_MARGIN, viewWidth - width - MENU_SCREEN_MARGIN);
  contextMenuState.y = clamp(rawY, MENU_SCREEN_MARGIN, viewHeight - height - MENU_SCREEN_MARGIN);
  contextMenuState.visible = true;
}

/** 菜单项激活：先收起再执行动作。 */
export async function activateContextMenuItem(id) {
  const entry = activeMenuActions.get(id);
  dismissMenu();
  if (entry?.enabled) {
    await entry.action?.(entry.context);
  }
}

export function initializeContextMenuService() {
  if (initialized) return;
  initialized = true;

  asyncListeners.add(
    addDomListener(document, "contextmenu", (event) => {
      if (event.defaultPrevented) return;
      openContextMenu(event);
    }),
  );

  // Click-to-dismiss：监听主窗口上的指针交互收起菜单。菜单条目点击也落在
  // document 上，需要在捕获阶段放行，否则条目在 click 前就被收起。
  asyncListeners.add(
    addDomListener(
      document,
      "mousedown",
      (event) => {
        if (event.button === 2) return;
        if (event.target instanceof Element && event.target.closest(".context-menu-panel")) {
          return;
        }
        if (contextMenuState.visible) {
          dismissMenu();
        }
      },
      true,
    ),
  );
}

export function dismissContextMenu() {
  dismissMenu();
}

export function openContextMenu(
  nativeEvent,
  { items = [], suppressDefaultEditItems = false } = {},
) {
  initializeContextMenuService();

  const context = buildEditContext(nativeEvent);
  const providedItems = normalizeItems(items);
  const shouldShowEditItems =
    !suppressDefaultEditItems &&
    (!providedItems.length || context.editableTarget || context.selectedText);
  const nextItems = normalizeItems([
    ...providedItems,
    ...(providedItems.length && shouldShowEditItems ? [{ type: "separator" }] : []),
    ...(shouldShowEditItems ? defaultEditItems(context) : []),
  ]);

  if (!nextItems.length) {
    dismissMenu();
    return;
  }

  nativeEvent?.preventDefault?.();
  nativeEvent?.stopPropagation?.();

  openMenu(nextItems, context, nativeEvent);
}
