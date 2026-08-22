# XTerm 脚本编写指南

XTerm 内置 JavaScript 终端自动化脚本引擎，可以模拟人工在终端中输入命令、等待设备输出、弹出交互表单收集参数，适合交换机/路由器批量开局、巡检等"大量重复 + 少量变化"的场景。

## 快速开始

1. 侧边栏进入 **脚本库**，点击 **添加脚本**，填写脚本名、作者、作者主页、备注（作者与主页会被记住）。
2. 在编辑器中编写脚本，内容自动保存。
3. 连接一个终端会话，点击终端工具栏的 **运行脚本** 按钮，选择要执行的脚本。

脚本在活动终端上以"模拟人工输入"的方式执行：输入内容会像真人键入一样在终端回显，并沿正常输入链路发送到后端设备。

## 脚本头（元数据）

每个脚本以 `==XTermScript==` 头块开头（油猴风格），脚本库的名称、作者、备注、版本等信息都从这里解析——直接改头块即改元数据：

```js
// ==XTermScript==
// @name        交换机开局配置
// @author      zhangsan
// @homepage    https://example.com/zhangsan
// @description 批量下发主机名与管理 VLAN
// @version     1.0.0
// @updateURL   https://example.com/scripts/provision.js
// ==/XTermScript==
```

| 字段 | 说明 |
| --- | --- |
| `@name` | 脚本名（必填，新建弹窗会写入） |
| `@author` | 作者 |
| `@homepage` | 作者主页（脚本库中可点击打开） |
| `@description` | 备注/用途说明 |
| `@version` | 版本号，点分数字（如 `1.2.0`），用于更新比较 |
| `@updateURL` | 更新检测地址，指向同一份脚本的远程版本（可选） |

### 更新检测

带 `@updateURL` 的脚本支持油猴式更新：脚本库工具栏点击 **检查更新**，或在 **设置 → 通用 → 脚本** 配置自动检测间隔（关闭 / 每 12 小时 / 每天 / 每周）。远程 `@version` 更高时卡片会显示"可更新至 vX"，点击更新按钮即整体替换为远程版本。

### 导入与导出

- **导入**：脚本库工具栏 → 导入，选择本地 `.js` 文件；头块中的元数据会自动解析，缺省时以文件名作为脚本名。
- **导出**：脚本卡片 → 导出，经原生保存对话框写为 `.js` 文件。

## 脚本 API

脚本以 async 函数体执行，可直接使用顶层 `await`。终端 API 统一通过 `xterm` 访问。

### 发送输入

```js
await xterm.send("system-view");      // 发送文本，不自动回车
await xterm.sendLine("sysname SW1");  // 发送一行（自动追加回车 \r）

// 发送命名按键：中断命令、菜单导航等场景
await xterm.press("ctrl+c");          // 中断正在运行的命令（\x03）
await xterm.press("esc");             // 退出菜单
await xterm.press("up");              // 方向键 ↑
```

`press()` 支持的按键：`enter`、`tab`、`esc`（或 `escape`）、`backspace`、`space`、方向键 `up`/`down`/`left`/`right`、`home`/`end`、`pageup`/`pagedown`、`insert`/`delete`、`f1`-`f12`，以及 `ctrl+<字母>`（如 `ctrl+c`、`ctrl-z`，`+`/`-` 分隔均可，不区分大小写）。未知名称会抛出 `TypeError`，不会静默发送错误序列。

### 等待与读取输出

```js
// 等待输出中出现指定文本或正则，返回匹配到的内容；默认超时 10s
await xterm.waitFor("[SW1]", 5000);

// 第三参数自定义超时错误信息（缺省使用内置 i18n 文案）
await xterm.waitFor("[SW1]", 5000, "未等到系统视图提示符");

// 等待多个模式中的任意一个
await xterm.waitForAny(["[SW1]", "[SW2]"], 5000);

// 收集 1 秒内的全部输出并返回
const text = await xterm.read(1000);

// 读取当前屏幕（可视区域）文本
const screen = await xterm.getScreen();
```

匹配基于剥离 ANSI 控制序列后的可读文本；已匹配的内容会被消费，后续 `waitFor` 不会重复命中同一段输出。

### 结构化匹配 `xterm.expect()` / `xterm.expectAny()`

`waitFor` 只返回匹配文本；需要**提取输出中的数值**或**拿到匹配前的命令回显**时，用结构化版本：

```js
// 发送命令并捕获提示符之间的完整输出
await xterm.sendLine("display version");
const hit = await xterm.expect(/uptime is (\d+) days/, 8000);
// hit = { text, groups, before }
//   text   — 匹配到的文本（同 waitFor 返回值）
//   groups — 正则捕获组数组（字符串模式时为 []）
//   before — 自上次匹配消费点到本次匹配之间收到的输出（命令回显 + 命令结果）
xterm.log("运行天数", hit.groups[0]);

// 等待多个模式并区分命中分支：index 为数组中的模式序号
const branch = await xterm.expectAny(["[SW1]", /Error|Failed/], 5000);
if (branch.index === 1) {
  throw new Error(`命令报错：${branch.text}`);
}
```

`expectAny` 在多个模式同时可命中时，**输出中位置最早者胜出**；位置相同则数组中靠前的模式优先。超时与消费语义与 `waitFor` 完全一致（默认超时 10s，`timeout <= 0` 表示永不超时，第三参数可自定义超时错误信息）。

### 等待

```js
await xterm.sleep(500);  // 毫秒
```

### 交互弹窗

```js
// 单输入框；支持 defaultValue / placeholder / required / type / pattern / errorMessage
const name = await xterm.input("请输入主机名", { defaultValue: "SW1", required: true });

// 确认框，返回 true / false
const ok = await xterm.confirm("确认下发配置？");

// 提示框
await xterm.alert("配置完成");
```

**取消 input/form 弹窗 = 取消整个脚本的执行**（运行状态记为"已停止"）；confirm 的取消是否定回答 `false`。

弹窗消息支持两种排版（input / confirm / alert / form 均适用）：

- 纯文本消息中的 `\n` 按换行渲染，无需额外处理。
- 传 `html: true` 时消息按 HTML 渲染，可输出富文本提示。渲染前会经严格白名单消毒：仅保留排版类标签（`p` / `br` / `b` / `strong` / `i` / `em` / `u` / `s` / `code` / `pre` / `span` / `div` / `ul` / `ol` / `li` / `blockquote` / `h1`-`h4` / `hr` / `table` 系列 / `a`），属性只保留安全协议（http/https/mailto）的 `a[href]`；`script` / `style` / `iframe` / 表单控件 / 事件属性等一律剥除，可防 XSS。

```js
await xterm.alert("第一条\n第二条");  // 纯文本换行

await xterm.alert(
  `<b>配置完成</b><ul><li>已下发 <code>${lines.length}</code> 行</li></ul><a href="https://example.com/runbook">查看手册</a>`,
  { html: true },
);
```

### 自定义表单 `xterm.form()`

一次弹出多字段表单，提交后返回 `{ key: value }` 对象：

```js
const values = await xterm.form({
  title: "开局配置",
  message: "请填写本台设备参数",   // 可选，显示在标题下方
  fields: [
    { key: "hostname", label: "主机名", defaultValue: "SW1", required: true },
    { key: "vlanId", label: "管理 VLAN", type: "number", placeholder: "例如 100" },
    { key: "password", label: "Enable 密码", type: "password", required: true },
    { key: "mode", label: "端口模式", type: "select", options: ["access", "trunk"], defaultValue: "access" },
    { key: "save", label: "完成后保存配置", type: "switch", defaultValue: true },
  ],
});
await xterm.sendLine(`sysname ${values.hostname}`);
```

#### 字段属性

| 属性 | 说明 |
| --- | --- |
| `key` | 字段键名（必填），提交后作为结果对象的属性名 |
| `label` | 显示标签，缺省用 `key` |
| `type` | `text`（默认）/ `password` / `select` / `switch` / `url` / `email` / `phone` / `number` |
| `defaultValue` | 默认值 |
| `placeholder` | 占位提示 |
| `options` | `select` 的选项数组（字符串或 `{ label, value }`） |
| `required` | 必填；未填点击确认会红字提醒并阻止提交 |
| `pattern` | 自定义验证正则（`RegExp` 或正则源码字符串），不匹配时报错 |
| `message` | 自定义错误文案，覆盖默认提示 |

#### 内置验证规则

| `type` | 规则 |
| --- | --- |
| `url` | 合法的 http/https URL |
| `email` | 邮箱格式 |
| `phone` | 中国大陆手机号（`1[3-9]` 开头 11 位） |
| `number` | 有效数字 |

自定义正则示例：

```js
{ key: "vlanId", label: "VLAN ID", required: true, pattern: /^\d{1,4}$/, message: "VLAN ID 必须是 1-4094 的数字" }
```

### 日志

```js
xterm.log("开始配置", values);  // 写入运行日志（对象自动 JSON 序列化）
```

### 本地文件读写

脚本可以把采集到的数据保存到本地文件，或读取本地文件作为输入参数。**读写路径都由用户在系统弹出的原生文件对话框中亲自选定**，脚本无法指定或探测任意路径；仅支持 UTF-8 文本数据（单文件上限 32 MB），不涉及任何命令执行。

```js
// 弹出打开对话框读取用户选定的文件，返回文件文本内容
const csv = await xterm.readFile();

// 弹出保存对话框把数据写入用户选定的位置，返回实际写入路径
// options.fileName 为建议文件名（默认 data.txt），options.title 可覆盖对话框标题
const path = await xterm.saveFile(csv, { fileName: "result.csv" });
xterm.log("已保存到", path);
```

**与 input/form 弹窗一致：取消文件对话框 = 取消整个脚本的执行**（运行状态记为"已停止"）。

典型用法——读取设备清单逐台下发的开局脚本，或把 `xterm.read()` 采集的巡检结果存档：

```js
const list = (await xterm.readFile())
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter(Boolean);

const results = [];
for (const host of list) {
  // …逐台处理，把结果推进 results…
}
await xterm.saveFile(results.join("\n"), { fileName: "巡检结果.txt" });
```

### 缓冲区读取与搜索

除了等待新输出，脚本还能直接读取终端**既有**内容（滚动回退 + 当前屏幕），只读、无 UI 副作用：

```js
// 整个缓冲区的文本（尾部空行已裁剪）
const buffer = await xterm.getBuffer();

// 逐行检索缓冲区：字符串按子串匹配，也支持 RegExp
// 返回 { count, matches: [{ line, text }] }；matches 最多带回 100 条，count 为命中总行数
const result = await xterm.searchBuffer(/error|fail/i);
if (result.count > 0) {
  xterm.log("发现异常行", result.count, result.matches[0]);
}
```

`getBuffer()` 配合 `saveFile()` 即可把"终端中显示的全部内容"归档；`searchBuffer()` 适合巡检脚本做事后断言。

### 会话记录控制

脚本可以直接启停**会话记录**（与工具栏"记录会话"同一条管线：输入+输出、ANSI 归一化、自动落盘），适合"执行一批命令并把全过程存档"的场景：

```js
// 弹出原生保存对话框选择记录文件（路径由用户决定），返回写入路径
const path = await xterm.startRecording();

await xterm.sendLine("display current-configuration");
await xterm.waitFor("#", 10000);

// 冲刷并收尾文件，返回写入路径；await xterm.isRecording() 可随时查询状态
await xterm.stopRecording();
await xterm.alert(`配置已保存到 ${path}`);
```

**取消保存对话框 = 取消整个脚本的执行**。脚本正常结束不会自动停止记录——未调用 `stopRecording()` 时记录会继续，可在终端工具栏手动停止；脚本被中途停止且记录对话框还悬停时，刚开始的记录会自动回滚。

### 会话信息

```js
xterm.session.id;        // 目标会话 id
xterm.session.label;     // 会话显示名
xterm.session.protocol;  // 连接协议（ssh / telnet / serial 等）
xterm.session.host;      // 目标地址（串口连接为空串）
xterm.session.port;      // 端口（串口连接为串口名，如 "COM3"）
xterm.session.username;  // 登录用户名
```

仅暴露非敏感连接信息，便于脚本按设备类型/协议分支；密码、私钥等凭证不会进入脚本作用域。典型用法——同一套巡检脚本按协议选择不同的提示符与分页退出方式：

## 错误处理与停止

- `waitFor` 超时、目标会话断开都会抛出错误，未捕获时脚本以"出错"结束并 toast 提示。
- 需要自行兜底时用 `try/catch`：

```js
try {
  await xterm.waitFor("[SW1]", 3000);
} catch (error) {
  xterm.log("未进入系统视图，中止", String(error));
  throw error;
}
```

## 完整示例：交换机批量开局

```js
// ==XTermScript==
// @name        交换机开局配置
// @author      zhangsan
// @description 主机名 + 管理 VLAN + 保存配置
// @version     1.1.0
// ==/XTermScript==

const values = await xterm.form({
  title: "开局配置",
  fields: [
    { key: "hostname", label: "主机名", defaultValue: "SW1", required: true },
    { key: "vlanId", label: "管理 VLAN", type: "number", required: true, pattern: /^\d{1,4}$/, message: "VLAN 必须是 1-4094 的数字" },
    { key: "save", label: "完成后保存配置", type: "switch", defaultValue: true },
  ],
});

await xterm.sendLine("system-view");
await xterm.waitFor("]", 5000, "未进入系统视图");

await xterm.sendLine(`sysname ${values.hostname}`);
await xterm.waitFor(`[${values.hostname}]`, 5000);

await xterm.sendLine(`vlan ${values.vlanId}`);
await xterm.waitFor(`[${values.hostname}-vlan${values.vlanId}]`, 5000);
await xterm.sendLine("quit");

if (values.save) {
  await xterm.sendLine("save");
  await xterm.waitForAny(["Y/N", "yes/no"], 5000);
  await xterm.sendLine("y");
  await xterm.waitFor("successfully", 8000, "保存配置失败");
}

await xterm.alert(`${values.hostname} 开局完成`);
xterm.log("开局配置完成", values);
```

## 安全模型与沙盒

脚本主体在**独立的 Web Worker 线程**中执行：Worker 内天然没有 DOM、没有 `__TAURI__` 桥、没有本地存储；脚本只能通过 RPC 调用主线程提供的 `xterm.*` 能力，以及使用安全的标准 JS 功能（`JSON`、`Math`、`URL`、`TextEncoder`、`atob`、`structuredClone`、`Promise` 等）。

这一架构带来三层硬性保护：

- **死循环不再卡死界面**：`while (true) {}` 只会占用 Worker 线程，界面保持响应，点击"停止脚本"即 `terminate()` 强杀；Worker 退出后其全部内存被整体回收。
- **能力隔离**：脚本触碰不到应用内部状态与任意后端命令，能用的只有文档列出的 `xterm.*` 函数。
- **网络隔离**：Worker 继承应用 CSP（`connect-src` 不允许外联任意主机），且沙盒同时屏蔽了 `fetch`/`WebSocket` 等全局。

**被禁用的全局能力**（Worker 内仍存在的那部分，访问即抛出点名该 API 的错误）：网络（`fetch`、`XMLHttpRequest`、`WebSocket`、`EventSource`、嵌套 `Worker` 等）、全局对象（`self`、`globalThis` 等）、Worker 内存储（`indexedDB`、`caches`）、动态代码生成（`Function`、模块加载）。

**需要用户授权的能力**一律走专用函数，由系统原生对话框决定具体目标，脚本无法指定路径或静默执行：

| 能力 | 函数 | 授权方式 |
| --- | --- | --- |
| 读取本地文件 | `xterm.readFile()` | 原生打开对话框选定文件 |
| 保存数据到文件 | `xterm.saveFile()` | 原生保存对话框选定路径 |
| 会话记录落盘 | `xterm.startRecording()` | 原生保存对话框选定记录文件 |
| 收集用户输入 | `xterm.input()` / `xterm.form()` | 应用内弹窗 |

所有授权对话框的**取消 = 停止脚本执行**。

**已知边界（请知悉）**：沙盒是同语言运行时的尽力隔离——刻意构造的脚本理论上仍可能经原型链逃逸触及 Worker 的真实全局（如 `fetch` 本体），但此时仍受 CSP 网络封锁，且 DOM / Tauri 命令在 Worker 中根本不存在。导入第三方脚本前仍应审查内容。

> 注：`getScreen`/`getBuffer`/`searchBuffer`/`isRecording` 在 Worker 架构下为异步 RPC，请始终 `await`（对旧脚本中同步写法的影响：不加 `await` 会得到 Promise 而非文本）。

## 注意事项

- 脚本在**当前活动终端**上执行；目标会话断开时输入会报错。
- 后台标签页约 30 秒后会挂起输出通道，`waitFor`/`read` 收不到新输出，执行脚本时请保持该终端在前台；`getBuffer`/`searchBuffer` 读取的是本地缓冲区，不受挂起影响。
- 脚本默认只能操作当前终端；网络、文件、存储等能力被沙盒禁用或需授权弹窗，详见上方"安全模型与沙盒"。导入第三方脚本前仍建议审查内容。
- 文件读写（`readFile`/`saveFile`）每次都会弹出系统对话框由用户确认具体文件，脚本无法在后台静默访问文件系统。
