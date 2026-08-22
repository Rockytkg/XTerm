// 脚本桥接 addon：把脚本引擎需要的能力收敛为标准 xterm 插件 API。
// - send 等同用户键入：走 xterm input() 触发 onData，输入在前端回显并沿正常链路
//   发往后端（xterm 6 的 paste() 只写显示端、不会触发 onData，不能用它）。
//   不绕过 trzsz 过滤、more-prompt 清理等既有输入处理。
// - 屏幕/光标行直接读 xterm buffer。
// - 输出由 TerminalPanel 在数据写入终端时经 notifyOutput 喂入（与渲染同源），
//   脚本侧的 waitFor/read 通过 onOutput 订阅。
export class ScriptBridgeAddon {
  constructor() {
    this._terminal = null;
    this._outputListeners = new Set();
  }

  activate(terminal) {
    this._terminal = terminal;
  }

  dispose() {
    this._terminal = null;
    this._outputListeners.clear();
  }

  send(data) {
    const text = String(data ?? "");
    if (!text || !this._terminal) return false;
    this._terminal.input(text, true);
    return true;
  }

  _readBufferLines(fromRow, toRow, trimRight) {
    const buffer = this._terminal?.buffer?.active;
    if (!this._terminal || !buffer) return [];
    const lines = [];
    for (let row = fromRow; row < toRow; row += 1) {
      const line = buffer.getLine(row);
      lines.push(line ? line.translateToString(trimRight) : "");
    }
    return lines;
  }

  getScreenText() {
    const buffer = this._terminal?.buffer?.active;
    if (!this._terminal || !buffer) return "";
    return this._readBufferLines(
      buffer.viewportY,
      buffer.viewportY + this._terminal.rows,
      false,
    ).join("\n");
  }

  // 整个缓冲区（滚动回退 + 当前屏幕）：右trim 每行并丢弃尾部空行，
  // 供脚本导出/检索终端既有内容。
  getBufferText() {
    const buffer = this._terminal?.buffer?.active;
    if (!this._terminal || !buffer) return "";
    const lines = this._readBufferLines(0, buffer.length, true);
    while (lines.length && !lines[lines.length - 1]) lines.pop();
    return lines.join("\n");
  }

  notifyOutput(data) {
    if (!data || !this._outputListeners.size) return;
    for (const listener of [...this._outputListeners]) listener(data);
  }

  onOutput(listener) {
    if (typeof listener !== "function") return () => {};
    this._outputListeners.add(listener);
    return () => {
      this._outputListeners.delete(listener);
    };
  }
}
