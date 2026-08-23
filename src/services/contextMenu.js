import { emitTo, listen } from "@tauri-apps/api/event";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { availableMonitors, cursorPosition } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { reactive } from "vue";
import {
  readText as readClipboardText,
  writeText as writeClipboardText,
} from "@tauri-apps/plugin-clipboard-manager";
import { currentLocale as getCurrentLocale, i18n } from "../i18n";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { addDomListener } from "../utils/domListeners";
import { noop } from "../utils/noop";
import {
  CONTEXT_MENU_EVENTS,
  CONTEXT_MENU_LAYOUT,
  CONTEXT_MENU_WINDOW_LABEL,
  shouldUseDomContextMenu,
} from "../utils/contextMenu";
import { isMacPlatform } from "../utils/platform";
import { getDesktopEnvironment } from "./desktopEnvironment";

const PRESERVE_DISABLED_IDS = new Set(["global-cut", "global-copy", "global-paste"]);
const MONITOR_CACHE_MS = 1000;
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
let popupWindow = null;
let popupReady = false;
let popupRequestId = 0;
let activeMenuActions = new Map();
let popupWindowPromise = null;
let warmPopupTimer = 0;
let popupMenuVisible = false;
let cachedMonitors = null;
let cachedMonitorsAt = 0;
let popupReadyWaiter = Promise.resolve();
let resolvePopupReadyWaiter = null;
let menuBackendPromise = null;
let lastEnvironment = null;
let forceDomMenu = false;
const asyncListeners = createAsyncListenerRegistry();

/**
 * DOM 降级菜单的渲染状态（Wayland 等无法用独立窗口定位的环境使用）。
 * 只存纯展示数据；动作回调留在非响应式的 activeMenuActions 里。
 */
export const domContextMenuState = reactive({
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

function positiveNumber(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : fallback;
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

/**
 * 菜单项的纯展示视图（剥离动作回调），悬浮窗口载荷与 DOM 降级菜单共用。
 * labelKey 一并带上：悬浮窗口里用于本地化重译，DOM 菜单里忽略即可。
 */
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

function menuPayload(items, requestId, { width, maxHeight }) {
  return {
    requestId,
    locale: getCurrentLocale(),
    theme: document.documentElement.dataset.theme === "dark" ? "dark" : "light",
    width,
    maxHeight,
    items: items.map(menuItemView),
  };
}

function contextMenuWindowUrl() {
  const entry = import.meta.env.DEV ? "/" : "index.html";
  return `${entry}?window=${encodeURIComponent(CONTEXT_MENU_WINDOW_LABEL)}`;
}

function waitForPopupReady() {
  return popupReady ? Promise.resolve() : popupReadyWaiter;
}

function resolvePopupReady() {
  if (popupReady) return;
  popupReady = true;
  resolvePopupReadyWaiter?.();
  resolvePopupReadyWaiter = null;
  popupReadyWaiter = Promise.resolve();
}

function resetPopupReadyWaiter() {
  popupReadyWaiter = new Promise((resolve) => {
    resolvePopupReadyWaiter = resolve;
  });
}

function resetPopupWindow() {
  popupWindow = null;
  popupReady = false;
  popupWindowPromise = null;
  popupMenuVisible = false;
  resolvePopupReadyWaiter?.();
  resetPopupReadyWaiter();
}

async function listAvailableMonitors() {
  const now = performance.now();
  if (cachedMonitors && now - cachedMonitorsAt < MONITOR_CACHE_MS) {
    return cachedMonitors;
  }
  cachedMonitors = await availableMonitors();
  cachedMonitorsAt = now;
  return cachedMonitors;
}

function createPopupWindow() {
  popupReady = false;
  resetPopupReadyWaiter();
  popupWindow = new WebviewWindow(CONTEXT_MENU_WINDOW_LABEL, {
    url: contextMenuWindowUrl(),
    title: "Context menu",
    width: MENU_WIDTH,
    height: 44,
    x: -10000,
    y: -10000,
    decorations: false,
    transparent: true,
    visible: false,
    resizable: false,
    maximizable: false,
    minimizable: false,
    closable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    shadow: true,
    focus: false,
    focusable: false,
  });
  popupWindow.once("tauri://error", resetPopupWindow);
  return popupWindow;
}

async function ensurePopupWindow() {
  const currentPopupWindow = await WebviewWindow.getByLabel(CONTEXT_MENU_WINDOW_LABEL);
  if (currentPopupWindow) {
    popupWindow = currentPopupWindow;
    popupReady = true;
    popupWindowPromise = Promise.resolve(currentPopupWindow);
  } else {
    resetPopupWindow();
    popupWindowPromise = Promise.resolve(createPopupWindow());
  }

  await popupWindowPromise;
  if (!popupReady) await waitForPopupReady();
  if (!popupWindow) return null;
  await popupWindow.setBackgroundColor([0, 0, 0, 0]).catch(noop);
  await popupWindow.setShadow(true).catch(noop);
  return popupWindow;
}

function warmPopupWindow() {
  window.clearTimeout(warmPopupTimer);
  warmPopupTimer = window.setTimeout(() => {
    warmPopupTimer = 0;
    ensurePopupWindow().catch(noop);
  }, 0);
}

function monitorForPosition(monitors, position) {
  return (
    monitors.find((monitor) => {
      const left = monitor.position.x;
      const top = monitor.position.y;
      const right = left + monitor.size.width;
      const bottom = top + monitor.size.height;
      return position.x >= left && position.x < right && position.y >= top && position.y < bottom;
    }) || monitors[0]
  );
}

function monitorBounds(monitor) {
  const area = monitor?.workArea || monitor;
  const areaPosition = area?.position || area;
  const areaSize = area?.size || area;
  return {
    x: finiteNumber(areaPosition?.x, monitor?.position?.x ?? 0),
    y: finiteNumber(areaPosition?.y, monitor?.position?.y ?? 0),
    width: positiveNumber(areaSize?.width, monitor?.size?.width ?? 0),
    height: positiveNumber(areaSize?.height, monitor?.size?.height ?? 0),
    scaleFactor: positiveNumber(monitor?.scaleFactor, 1),
  };
}

async function popupGeometry(width, items) {
  const position = await cursorPosition();
  const monitors = await listAvailableMonitors();
  const monitor = monitorForPosition(monitors, position);
  if (!monitor) {
    return {
      height: menuHeight(items),
      maxHeight: MENU_MAX_HEIGHT,
      position,
    };
  }

  const bounds = monitorBounds(monitor);
  const margin = MENU_SCREEN_MARGIN;
  const maxPhysicalHeight = Math.max(
    MENU_MIN_HEIGHT * bounds.scaleFactor,
    bounds.height - margin * 2,
  );
  const maxLogicalHeight = Math.floor(maxPhysicalHeight / bounds.scaleFactor);
  const maxHeight = Math.min(MENU_MAX_HEIGHT, Math.max(MENU_MIN_HEIGHT, maxLogicalHeight));
  const height = menuHeight(items, maxHeight);
  const physicalWidth = Math.ceil(width * bounds.scaleFactor);
  const physicalHeight = Math.ceil(height * bounds.scaleFactor);
  const left = bounds.x + margin;
  const top = bounds.y + margin;
  const right = bounds.x + bounds.width - physicalWidth - margin;
  const bottom = bounds.y + bounds.height - physicalHeight - margin;

  return {
    height,
    maxHeight,
    position: new PhysicalPosition(clamp(position.x, left, right), clamp(position.y, top, bottom)),
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

/**
 * 菜单渲染后端：默认独立悬浮窗口；Wayland 会话用 DOM 菜单
 * （见 shouldUseDomContextMenu）。forceDomMenu 是悬浮窗口运行期失败后的
 * 永久降级开关。
 */
function resolveMenuBackend() {
  if (!menuBackendPromise) {
    menuBackendPromise = getDesktopEnvironment().then((environment) => {
      lastEnvironment = environment;
      return shouldUseDomContextMenu(environment) ? "dom" : "window";
    });
  }
  return menuBackendPromise.then((backend) => (forceDomMenu ? "dom" : backend));
}

function hidePopupMenu() {
  if (!popupMenuVisible) return;
  popupMenuVisible = false;
  popupWindow?.hide?.().catch(noop);
}

function hideDomMenu() {
  if (!domContextMenuState.visible) return;
  domContextMenuState.visible = false;
  domContextMenuState.items = [];
}

function hideAnyMenu() {
  activeMenuActions = new Map();
  hideDomMenu();
  hidePopupMenu();
}

/**
 * 在主窗口内打开 DOM 菜单。定位用 contextmenu 事件的 clientX/Y（窗口内
 * 坐标，Wayland 下可靠），并按视口边缘收拢。
 */
function openDomMenu(items, context, nativeEvent) {
  popupRequestId += 1;
  hideAnyMenu();
  activeMenuActions = menuActionMap(items, context);
  const width = MENU_WIDTH;
  const height = menuHeight(items);
  const viewWidth = Math.max(window.innerWidth || 0, width + MENU_SCREEN_MARGIN * 2);
  const viewHeight = Math.max(window.innerHeight || 0, height + MENU_SCREEN_MARGIN * 2);
  const rawX = finiteNumber(nativeEvent?.clientX, (viewWidth - width) / 2);
  const rawY = finiteNumber(nativeEvent?.clientY, (viewHeight - height) / 2);
  domContextMenuState.items = items.map(menuItemView);
  domContextMenuState.theme = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  domContextMenuState.width = width;
  domContextMenuState.maxHeight = height;
  domContextMenuState.x = clamp(rawX, MENU_SCREEN_MARGIN, viewWidth - width - MENU_SCREEN_MARGIN);
  domContextMenuState.y = clamp(rawY, MENU_SCREEN_MARGIN, viewHeight - height - MENU_SCREEN_MARGIN);
  domContextMenuState.visible = true;
}

/** DOM 菜单项激活：与悬浮窗口路径一致，先收起再执行动作。 */
export async function activateDomContextMenuItem(id) {
  const entry = activeMenuActions.get(id);
  hideAnyMenu();
  if (entry?.enabled) {
    await entry.action?.(entry.context);
  }
}

async function openPopupMenu(items, context) {
  if (!items.length) {
    hideAnyMenu();
    return true;
  }
  const requestId = ++popupRequestId;
  const windowWidth = MENU_WIDTH;

  try {
    activeMenuActions = menuActionMap(items, context);
    const [menuWindow, geometry] = await Promise.all([
      ensurePopupWindow(),
      popupGeometry(windowWidth, items),
    ]);
    if (!menuWindow) return false;
    if (requestId !== popupRequestId) return true;

    await menuWindow.setSize(new LogicalSize(windowWidth, geometry.height));
    await menuWindow.setPosition(geometry.position);
    if (requestId !== popupRequestId) return true;

    await emitTo(
      CONTEXT_MENU_WINDOW_LABEL,
      CONTEXT_MENU_EVENTS.open,
      menuPayload(items, requestId, {
        width: windowWidth,
        maxHeight: geometry.maxHeight,
      }),
    );
    if (requestId !== popupRequestId) return true;

    await menuWindow.show();
    popupMenuVisible = true;
    return true;
  } catch {
    resetPopupWindow();
    popupMenuVisible = false;
    activeMenuActions = new Map();
    return false;
  }
}

async function runPopupMenuAction(payload = {}) {
  if (payload.requestId !== popupRequestId) return;
  const item = activeMenuActions.get(payload.id);
  hideAnyMenu();
  if (item?.enabled) {
    await item.action?.(item.context);
  }
}

export function initializeContextMenuService() {
  if (initialized) return;
  initialized = true;

  asyncListeners.add(
    addDomListener(document, "contextmenu", (event) => {
      if (event.defaultPrevented) return;
      void openContextMenu(event);
    }),
  );

  // Click-to-dismiss: the context menu is a separate Tauri window with focus: false,
  // so it never receives/loses focus and onFocusChanged-based blur detection never
  // fires.  Listen for pointer interactions on the main window and dismiss the
  // popup when the user clicks (any button except the right button, which may be
  // repositioning the menu).  DOM 降级菜单（Wayland）的条目点击也落在主窗口
  // document 上，需要在捕获阶段放行，否则条目在 click 前就被收起。
  asyncListeners.add(
    addDomListener(
      document,
      "mousedown",
      (event) => {
        if (event.button === 2) return;
        if (event.target instanceof Element && event.target.closest(".dom-context-menu-panel")) {
          return;
        }
        if (popupMenuVisible || domContextMenuState.visible) {
          hideAnyMenu();
        }
      },
      true,
    ),
  );
  asyncListeners.register(listen(CONTEXT_MENU_EVENTS.ready, resolvePopupReady)).then(() => {
    // Wayland 下根本不会用悬浮窗口，预热只会留下一个隐藏的空窗口。
    void resolveMenuBackend().then((backend) => {
      if (backend === "window") warmPopupWindow();
    });
  });
  asyncListeners.register(
    listen(CONTEXT_MENU_EVENTS.action, ({ payload }) => runPopupMenuAction(payload)),
  );
  asyncListeners.register(
    listen(CONTEXT_MENU_EVENTS.close, ({ payload }) => {
      if (payload?.requestId === popupRequestId) {
        hideAnyMenu();
      }
    }),
  );
}

export function dismissContextMenu() {
  hideAnyMenu();
}

export async function openContextMenu(
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
    hideAnyMenu();
    return;
  }

  nativeEvent?.preventDefault?.();
  nativeEvent?.stopPropagation?.();

  const backend = await resolveMenuBackend();
  if (backend === "dom") {
    openDomMenu(nextItems, context, nativeEvent);
    return;
  }

  const opened = await openPopupMenu(nextItems, context);
  if (!opened && lastEnvironment?.platform === "linux") {
    // 悬浮窗口创建/定位失败（部分 Wayland 合成器直接拒绝透明无边框窗口）：
    // 之后一律改用 DOM 菜单，不再反复尝试失败路径。
    forceDomMenu = true;
    openDomMenu(nextItems, context, nativeEvent);
  }
}
