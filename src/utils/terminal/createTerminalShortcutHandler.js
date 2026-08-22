import { isMacPlatform } from "../platform.js";
import { createShortcutRegistry } from "../shortcutRegistry.js";

/**
 * 终端聚焦时的快捷键（复制/粘贴/中断/搜索），基于统一注册表实现。
 * 通过 xterm 的 attachCustomKeyEventHandler 分发：handleEvent 返回 false
 * 表示事件已消费，xterm 不再做默认处理。
 */
export function createTerminalShortcutHandler({
  copySelection,
  hasSelection,
  pasteClipboard,
  sendInterrupt,
  canOpenSearch,
  openSearch,
  searchShortcut,
  // 可注入便于测试；默认按运行平台判断。
  isMac = isMacPlatform(),
}) {
  const registry = createShortcutRegistry();
  registry.register({
    id: "terminal.copy",
    shortcut: "Ctrl+Shift+C",
    stopPropagation: true,
    run: copySelection,
  });
  registry.register({
    id: "terminal.copy-or-interrupt",
    shortcut: "Ctrl+C",
    stopPropagation: true,
    run: () => {
      if (hasSelection()) {
        copySelection();
        return;
      }
      sendInterrupt();
    },
  });
  registry.register({
    id: "terminal.paste",
    shortcut: "Ctrl+Shift+V",
    stopPropagation: true,
    run: pasteClipboard,
  });
  // macOS 沿用 Cmd 系习惯；中断仍是 Ctrl+C（与系统终端一致）。
  if (isMac) {
    registry.register({
      id: "terminal.mac-copy",
      shortcut: "Cmd+C",
      // 无选中时放行给系统处理，因此不 preventDefault、不 stopPropagation。
      preventDefault: false,
      run: () => {
        if (!hasSelection()) return "continue";
        copySelection();
      },
    });
    registry.register({
      id: "terminal.mac-copy-explicit",
      shortcut: "Cmd+Shift+C",
      stopPropagation: true,
      run: copySelection,
    });
    registry.register({
      id: "terminal.mac-paste",
      shortcut: "Cmd+V",
      stopPropagation: true,
      run: pasteClipboard,
    });
  }
  registry.register({
    id: "terminal.search",
    shortcut: () => searchShortcut(),
    when: () => canOpenSearch(),
    stopPropagation: true,
    run: openSearch,
  });
  return (event) => registry.handleEvent(event);
}
