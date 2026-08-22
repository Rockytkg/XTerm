// 新建脚本的默认正文：仅提供紧凑的注释式 API 速查，不执行任何终端操作。
export const DEFAULT_SCRIPT_BODY = `// 终端 API（异步函数请使用 await）：
// await xterm.send("system-view") — 发送文本，不自动追加回车。
// await xterm.sendLine("sysname SW1") — 发送一行并自动追加回车（\\r）。
// await xterm.waitFor("[SW1]", 5000, "超时提示") — 等待文本或正则匹配，可设置超时和错误信息。
// await xterm.waitForAny(["[SW1]", /SW2/], 5000) — 等待多个文本或正则模式中的任意一个。
// const hit = await xterm.expect(/vlan (\\d+)/, 5000) — 等待匹配并返回 { text, groups, before }（捕获组 + 匹配前的输出）。
// const branch = await xterm.expectAny(["[SW1]", /Error/], 5000) — 返回 { index, text, groups, before }，index 为命中的模式序号。
// await xterm.press("ctrl+c") — 发送命名按键（enter/esc/tab/方向键/f1-f12/ctrl+<字母>）。
// const output = await xterm.read(1000) — 收集指定时间内的终端输出。
// const screen = xterm.getScreen() — 读取终端当前可视区域文本。
// await xterm.sleep(500) — 等待指定毫秒数。
// const value = await xterm.input("请输入主机名", { defaultValue: "SW1", required: true }) — 显示输入框，取消会停止脚本。
// const confirmed = await xterm.confirm("确认下发配置？") — 显示确认框并返回 true 或 false。
// await xterm.alert("配置完成") — 显示提示框。
// const values = await xterm.form({ title: "参数", fields: [{ key: "hostname", label: "主机名", required: true }] }) — 显示多字段表单。
// xterm.log("运行日志", { ok: true }) — 写入脚本运行日志。
// xterm.session.id — 当前目标会话 ID。
// xterm.session.label — 当前目标会话显示名称。
// xterm.session.protocol / host / port / username — 非敏感连接信息（不含密码、私钥）。
// 支持标准 JavaScript、Promise、try/catch 和定时器；网络与文件等能力被沙盒限制，建议始终 await 异步 API。
`;
