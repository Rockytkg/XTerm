import { reactive } from "vue";
import {
  readText as readClipboardText,
  writeText as writeClipboardText,
} from "@tauri-apps/plugin-clipboard-manager";
import { createAsyncListenerRegistry } from "../utils/asyncListeners";
import { addDomListener } from "../utils/domListeners";
import {
  CONTEXT_MENU_LAYOUT,
  contextMenuHeight,
  contextMenuPosition,
  normalizeContextMenuItems,
} from "../utils/contextMenu";
import { isMacPlatform, isPrimaryModifier } from "../utils/platform";
import { createLogger } from "../utils/logger";

const logger = createLogger("frontend.context_menu.service");

const { maxHeight: MENU_MAX_HEIGHT, width: MENU_WIDTH } = CONTEXT_MENU_LAYOUT;

let initialized = false;
let activeMenuActions = new Map();
// 递增令牌：菜单被关闭或出现新的打开请求时，使进行中的异步打开（剪贴板读取）失效。
let openToken = 0;
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

function selectedTextFrom(target) {
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = target.selectionStart ?? 0;
    const end = target.selectionEnd ?? start;
    return target.value.slice(start, end);
  }
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
  return {
    editableTarget,
    nativeEvent,
    selection: editableTarget ? captureEditableSelection(editableTarget) : {},
    selectedText: selectedTextFrom(editableTarget),
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

/** 派发 input 事件，保证受控组件（:value + @input）状态与 DOM 同步。 */
function notifyInput(target) {
  target.dispatchEvent(new Event("input", { bubbles: true }));
}

function insertTextIntoTarget(target, text, context) {
  if (!target) return;

  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const start = context.selection?.selectionStart ?? target.selectionStart ?? target.value.length;
    const end = context.selection?.selectionEnd ?? target.selectionEnd ?? start;
    target.focus();
    target.setRangeText(text, start, end, "end");
    notifyInput(target);
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
    notifyInput(target);
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

/**
 * 撤销/重做：execCommand 在 Chromium / WebKit 的文本框与 contenteditable 上
 * 都能驱动原生编辑历史。菜单动作执行时菜单已关闭，先把焦点还回目标元素；
 * 部分 webview 撤销/重做后不派发 input，值变化时补发一次以同步受控组件。
 */
function runEditHistoryCommand(context, command) {
  const target = context.editableTarget;
  if (!target) return;

  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const before = target.value;
    target.focus();
    document.execCommand?.(command);
    if (target.value !== before) notifyInput(target);
    return;
  }

  if (target.isContentEditable) {
    restoreContentEditableRange(target, context);
    document.execCommand?.(command);
  }
}

/**
 * 撤销/重做快捷键兜底：WebKitGTK 等 webview 不为表单控件处理编辑类加速键，
 * 统一在这里补齐；webview 原生已处理时 execCommand 与原生行为等价
 * （preventDefault 后不会重复执行）。元素级已处理（CodeMirror、xterm
 * 会 preventDefault）或终端区域内不介入，避免与各自的历史栈冲突。
 */
function handleEditHistoryShortcut(event) {
  if (event.defaultPrevented || event.altKey || !isPrimaryModifier(event)) return;
  // 优先用物理键位（code），避免非英文布局下 event.key 不是 z/y。
  const key =
    event.code === "KeyZ" || event.code === "KeyY"
      ? event.code.slice(-1).toLowerCase()
      : String(event.key || "").toLowerCase();
  const command =
    key === "z"
      ? event.shiftKey
        ? "redo"
        : "undo"
      : key === "y" && !event.shiftKey && !isMacPlatform()
        ? "redo"
        : null;
  if (!command) return;
  const target = editableTargetFrom(event.target);
  if (!target || target.closest(".xterm")) return;
  event.preventDefault();
  runEditHistoryCommand({ editableTarget: target }, command);
}

function defaultEditItems(context) {
  const hasSelection = !!context.selectedText;
  const canEdit = !!context.editableTarget;
  // macOS 上编辑快捷键是 Cmd 系，菜单文案同步显示 ⌘。
  const shortcutPrefix = isMacPlatform() ? "⌘" : "Ctrl+";

  return [
    ...(canEdit
      ? [
          {
            id: "global-undo",
            labelKey: "contextMenu.undo",
            icon: "undo",
            shortcut: `${shortcutPrefix}Z`,
            action: () => runEditHistoryCommand(context, "undo"),
          },
          {
            id: "global-redo",
            labelKey: "contextMenu.redo",
            icon: "redo",
            shortcut: isMacPlatform() ? "⇧⌘Z" : `${shortcutPrefix}Y`,
            action: () => runEditHistoryCommand(context, "redo"),
          },
          { type: "separator" },
        ]
      : []),
    {
      id: "global-cut",
      labelKey: "contextMenu.cut",
      icon: "cut",
      enabled: canEdit && hasSelection,
      shortcut: `${shortcutPrefix}X`,
      action: () => cutSelection(context),
    },
    {
      id: "global-copy",
      labelKey: "contextMenu.copy",
      icon: "copy",
      enabled: hasSelection,
      shortcut: `${shortcutPrefix}C`,
      action: () => copySelection(context),
    },
    {
      id: "global-paste",
      labelKey: "contextMenu.paste",
      icon: "paste",
      enabled: canEdit,
      shortcut: `${shortcutPrefix}V`,
      action: () => pasteIntoTarget(context),
    },
    {
      id: "global-delete",
      labelKey: "contextMenu.delete",
      icon: "delete",
      enabled: canEdit && hasSelection,
      action: () => deleteSelection(context),
    },
    { type: "separator" },
    {
      id: "global-select-all",
      labelKey: "contextMenu.selectAll",
      icon: "selectAll",
      enabled: canEdit || !!document.body?.innerText,
      shortcut: `${shortcutPrefix}A`,
      action: () => selectAll(context),
    },
  ];
}

function finiteNumber(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
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

function isContextMenuTarget(target) {
  return target instanceof Element && !!target.closest("[data-context-menu-root]");
}

function preserveModalForMenuInteraction(event) {
  if (isContextMenuTarget(event.target)) {
    // Reka UI 的 dismissable layer 在 document 冒泡阶段处理 pointerdown；
    // 菜单按钮自身已经完成目标阶段处理，此处只阻断后续的 outside-dismiss。
    event.stopImmediatePropagation();
  }
}

function dismissMenu() {
  openToken += 1;
  activeMenuActions = new Map();
  if (!contextMenuState.visible) return;
  contextMenuState.visible = false;
  contextMenuState.items = [];
}

/**
 * 打开菜单。定位用 contextmenu 事件的 clientX/Y（窗口内坐标），边缘翻转
 * 与收拢规则见 contextMenuPosition。
 */
function openMenu(items, context, nativeEvent) {
  dismissMenu();
  activeMenuActions = menuActionMap(items, context);
  const width = MENU_WIDTH;
  const height = contextMenuHeight(items);
  const viewWidth = window.innerWidth || 0;
  const viewHeight = window.innerHeight || 0;
  const { x, y } = contextMenuPosition({
    x: finiteNumber(nativeEvent?.clientX, (viewWidth - width) / 2),
    y: finiteNumber(nativeEvent?.clientY, (viewHeight - height) / 2),
    width,
    height,
    viewWidth,
    viewHeight,
  });
  contextMenuState.items = items.map(menuItemView);
  contextMenuState.theme = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  contextMenuState.width = width;
  contextMenuState.maxHeight = height;
  contextMenuState.x = x;
  contextMenuState.y = y;
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

  // 输入框撤销/重做快捷键兜底（见 handleEditHistoryShortcut 注释）。
  asyncListeners.add(addDomListener(document, "keydown", handleEditHistoryShortcut));

  // Click-to-dismiss：在捕获阶段收起菜单外的交互，菜单根节点内的事件继续
  // 传播到按钮，以便由 Vue 的 click 处理器执行动作。
  // 必须用 pointerdown 而不是 mousedown：Reka UI 的 DialogOverlay 会对落在自身的
  // 左键 pointerdown 调 preventDefault（DialogOverlayImpl 的 withModifiers prevent），
  // pointerdown 被取消后浏览器不再派发 mousedown/mouseup/click 兼容鼠标事件，
  // 遮罩点击若靠 mousedown 监听就永远收不到，模态框关了菜单却残留。
  asyncListeners.add(
    addDomListener(
      document,
      "pointerdown",
      (event) => {
        if (event.button === 2) return;
        if (isContextMenuTarget(event.target)) return;
        if (contextMenuState.visible) {
          dismissMenu();
        }
      },
      true,
    ),
  );

  // 菜单通过 Teleport 位于 DialogContent 外部，但仍属于当前交互链路。
  // 在 document 冒泡阶段隔离菜单事件，避免模态框误判为点击外部而关闭。
  asyncListeners.add(addDomListener(document, "pointerdown", preserveModalForMenuInteraction));
  asyncListeners.add(addDomListener(document, "click", preserveModalForMenuInteraction));

  // 与原生菜单一致：视口缩放或菜单外滚动时收起（菜单自身滚动除外）。
  asyncListeners.add(
    addDomListener(window, "resize", () => {
      if (contextMenuState.visible) dismissMenu();
    }),
  );
  asyncListeners.add(
    addDomListener(
      document,
      "scroll",
      (event) => {
        if (contextMenuState.visible && !isContextMenuTarget(event.target)) dismissMenu();
      },
      true,
    ),
  );
}

export function dismissContextMenu() {
  dismissMenu();
}

export async function openContextMenu(
  nativeEvent,
  { items = [], suppressDefaultEditItems = false } = {},
) {
  initializeContextMenuService();

  const context = buildEditContext(nativeEvent);
  const providedItems = normalizeContextMenuItems(items);
  const editItems =
    !suppressDefaultEditItems &&
    (!providedItems.length || context.editableTarget || context.selectedText)
      ? defaultEditItems(context)
      : [];
  const nextItems = normalizeContextMenuItems([
    ...providedItems,
    ...(providedItems.length && editItems.length ? [{ type: "separator" }] : []),
    ...editItems,
  ]);

  if (!nextItems.length) {
    dismissMenu();
    return;
  }

  nativeEvent?.preventDefault?.();
  nativeEvent?.stopPropagation?.();
  // 新的右键请求立即收起旧菜单：剪贴板读取是异步 IPC，期间旧菜单不应残留。
  dismissMenu();

  // “粘贴”的可用态取决于剪贴板是否真有文本：读取是异步 IPC，读取期间
  // 菜单被关闭或出现新的右键请求（openToken 变化）时放弃本次打开。
  const pasteItem = nextItems.find((item) => item.id === "global-paste" && item.enabled);
  if (pasteItem) {
    const token = ++openToken;
    const clipboardText = await readClipboardText().catch((error) => {
      // 读取失败时按无文本处理（禁用“粘贴”），只记录日志不打断菜单打开。
      logger.warn("context-menu.clipboard.read.failed", error);
      return "";
    });
    if (token !== openToken) return;
    pasteItem.enabled = !!clipboardText;
  }

  openMenu(nextItems, context, nativeEvent);
}
