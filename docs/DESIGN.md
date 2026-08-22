# XTerm 设计说明

## 设计目标

XTerm 是桌面终端工作区，不是营销页或展示型网站。界面应优先服务长期操作、快速扫描和可靠反馈。

核心目标：

- 保持工具型界面的密度和清晰度。
- 让连接、终端、SFTP、凭证和设置入口稳定可预期。
- 在亮色和暗色主题下保持一致的信息层级。
- 对连接失败、主机公钥确认、文件传输、会话记录等高风险操作给出明确状态。

## 当前界面架构

路由与页面（`src/router/`）：

- `src/layouts/AppShell.vue`：桌面窗口壳层，负责标题栏、主导航和路由出口。
- `SessionsView.vue`（`/sessions`）：连接列表、连接新增/编辑/删除和进入工作区。
- `DashboardView.vue`（`/workspace`）：活动工作区，由 `src/views/workspace/` 下的终端面板（`TerminalWorkspacePane.vue`）和按连接类型出现的 SFTP 面板（`SftpWorkspacePane.vue`）组成。
- `KeysView.vue`（`/keys`）：密码和 SSH 私钥凭证管理；`CredentialGraphView.vue`（`/credential-graph`）提供凭证关系图谱视图。
- `ScriptsView.vue`（`/scripts`）：脚本库，管理终端自动化脚本（新建、编辑、导入导出、更新检测）。
- `SettingsLayout.vue`（`/settings`）：设置壳层，子页面位于 `src/views/settings/`。

核心组件：

- `TerminalPanel.vue`：使用 XTerm.js 和 `src/utils/terminal/addons/` 下的 addon 实现真实终端体验。
- `SftpPanel.vue`：SSH 会话下的远程文件管理和传输反馈。

## 视觉语言

项目使用偏 Flyme 风格的轻量桌面工具设计，但当前实现以系统字体、SCSS 变量和 UnoCSS shortcut 为准。

基础原则：

- 背景分为页面、面板、输入/悬停三层。
- 文本分为主文本、次级文本、辅助文本三层。
- 强调色只用于主操作、选中态、焦点态和关键状态，不作为大面积装饰。
- 语义色只用于成功、警告、错误、信息状态。
- 终端区域可以更接近代码工具视觉，但仍需与应用主题协调。

## 样式来源

主要样式入口：

- `src/styles.scss`：设计 token、亮暗主题、reset 补充、原生控件修正和必须存在的全局基础规则。
- `uno.config.js`：颜色 token 映射、字体和阴影 token、可复用的 `ui-*` 原语与组合 shortcut。
- `@unocss/reset/tailwind.css`：基础 reset。
- `@xterm/xterm/css/xterm.css`：xterm.js 必需样式。

样式分层：

- Token 层：只在 `src/styles.scss` 中维护 `--bg-*`、`--text-*`、`--border-*`、`--radius-*`、`--shadow-*`、`--ease-*` 等设计 token。
- Primitive 层：在 `uno.config.js` 中定义 `ui-*` 共享原语，例如页面头部、空状态、行操作、侧栏分组、按钮和输入框。
- Feature shell 层：只为跨多个子元素、且在同一类功能内重复出现的结构保留 feature shortcut，例如 `overview-*`、`sftp-*`、`cred-*`。
- Page 层：页面组件尽量直接组合 token 和 `ui-*` 原语，不再为单页一次性结构新增全局 class。

新增样式时优先复用：

- `--bg-primary`、`--bg-secondary`、`--bg-tertiary`
- `--text-primary`、`--text-secondary`、`--text-tertiary`
- `--accent`、`--accent-hover`、`--accent-active`、`--accent-light`
- `--success`、`--warning`、`--danger`、`--info`
- `--border`、`--border-light`
- `--radius-sm`、`--radius-md`、`--radius-lg`、`--radius-xl`
- `--shadow-sm`、`--shadow-md`、`--shadow-lg`、`--shadow-xl`
- `--ease-default`、`--ease-enter`、`--ease-exit`

新增样式决策顺序：

- 先复用现有 token。
- 再复用已有 `ui-*` 原语。
- 如果多个页面或同一功能模块内多处重复，再提升到新的 shortcut。
- 只有原生伪元素、第三方库覆盖或确实无法用原子类表达时，才新增局部 `<style>` 或全局 SCSS。

## 布局规范

- 桌面端以应用壳层为主，不做独立 landing page。
- 左侧导航和顶部标题栏应保持稳定，避免页面间跳动。
- 工作区主区域优先给终端空间，辅助信息放入右侧概览或弹层。
- 连接列表、凭证列表和设置页应保持可扫描、可重复操作，不使用大段解释性文案。
- 布局基准与 Tauri 最小窗口一致，为 `960 x 600`；全局样式通过 `--viewport-min-inline` 和 `--viewport-min-block` 约束最小画布。
- 样式必须使用 `@layer` 管理级联顺序：`reset`、`tokens`、`base`、`components`、`overlays`、`utilities`。
- 色彩系统以 `oklch()` 与 `light-dark()` 为基础；主题自动模式由 CSS 色彩协商处理，不再使用 JS `matchMedia`。
- 尺寸过渡优先使用 CSS 变量和 `clamp()`，避免通过媒体查询或 JS 断点维护响应式状态。
- 组件级适配使用 Container Queries；禁止新增 `@media` 作为布局或主题分支。
- 弹层定位优先使用 Anchor Positioning，并保留基础定位回退；面板/路由切换优先使用 View Transitions API。
- 禁止新增百分比宽度工具类或内联百分比宽度；填充容器使用 logical size shortcut，例如 `ui-fill-inline`。
- 窗口最小尺寸由 `src-tauri/tauri.conf.json` 控制，当前为 `960 x 600`。

## 性能决策

当前 GUI 以极致性能为优先级，允许破坏旧版交互细节。

- SFTP 文件列表使用固定行高虚拟渲染，避免大目录下全量 DOM diff。

## 动效规范

动效服务于状态确认、空间连续性和错误预防，不承担装饰目的。应用级动效统一从 `src/utils/motion/index.js` 进入。

- 动效必须快速、轻量、可中断；默认 70/110/180ms 三档，位移控制在 2-5px。
- 弹窗、Toast、设置面板、路由、主题和侧栏切换必须使用统一 motion 模块。
- 终端输入、输出、滚动、尺寸同步和 xterm 渲染链路禁止接入应用级动效。
- SFTP 传输进度、加载旋转、按钮 hover/focus 等局部状态使用 CSS token，不写组件内散落时长。
- 图谱布局、拖拽排序等领域库自带运动只保留必要参数，不接入 GSAP。
- 所有应用级动效必须同时尊重系统 `prefers-reduced-motion` 和应用内 `enableAnimations` 偏好。

## 组件规范

- 按钮优先使用 lucide 图标和已有 `ui-button-*` shortcut。
- 图标继承当前文本颜色，避免硬编码图标色。
- 表单控件复用 `ui-input`、`UiSelect.vue` 和现有弹窗模式。
- 弹窗用于确认、连接配置、主机公钥确认等阻断式操作。
- Toast 用于保存成功、连接失败、文件传输、会话记录等短反馈。
- 不要在卡片中再套大卡片；列表项、弹窗和重复实体可以使用卡片边界。

## 终端设计

终端是核心体验，修改时需要优先保证可用性。

- 字体、字号、行高、内边距、光标和滚动行为由设置页偏好控制。
- 终端附加能力以 addon 形式组织在 `src/utils/terminal/addons/`：输出、尺寸同步、搜索、剪贴板（OSC 52）、桌面通知（OSC 9/777）、超链接、Unicode 11、连字、进度序列、关键字高亮、脚本桥接、trzsz 等默认加载；WebGL 渲染器保留独立开关。
- 工作区只保留当前活跃终端实例，切换会话时从输出快照恢复，避免隐藏终端长期占用渲染和内存资源；后台标签页约 30 秒后挂起输出通道，回到前台时恢复。
- 终端输出以更大的批量写入 xterm.js，优先降低高吞吐输出时的 WebView 刷新频率。
- 连接失败信息应写入终端并同步 Toast 或连接状态，不要只在控制台输出。
- 终端尺寸变化需要同步到 Rust 后端会话，避免远端 TTY 尺寸错误。
- 搜索、复制、清屏、快捷输入和会话记录应保持键盘与鼠标都可操作。

## 关键字高亮设计

运行时关键字高亮按"方案 → 规则"组织，方案绑定终端主题，并在连接的会话选项中按连接开启。规则渲染只覆盖可视区域（上下各 4 行 overscan、每行最多 32 个匹配、按帧分批扫描），避免 `onWriteParsed`、`onRender` 和滚动监听在全量缓冲区上持续占用主线程。

- 方案与规则的管理 UI 位于设置页"关键字高亮"（`KeywordHighlightSettingsView.vue` / `KeywordHighlightSchemeEditor.vue`）。
- 每个终端主题只能归属一个方案；未绑定主题的规则不会生效。
- 方案导入/导出走原生文件对话框，导入会整体替换现有方案列表。
- 用户使用文档见 `docs/HIGHLIGHTING.md`，示例方案见 `examples/highlight-schemes/`。

## 脚本引擎设计

终端自动化脚本在独立的 Web Worker 沙盒中执行，通过 RPC 调用主线程提供的 `xterm.*` 能力；网络、存储、动态代码生成等全局能力在 Worker 内被屏蔽，文件读写、会话记录等敏感能力一律经原生对话框由用户授权。

- 脚本主体与 UI 隔离：死循环只占用 Worker 线程，"停止脚本"即 `terminate()` 强杀。
- 脚本元数据采用油猴风格 `==XTermScript==` 头块，脚本库直接解析头块。
- 完整 API、安全模型与示例见 `docs/SCRIPTING.md`，可直接导入的示例脚本见 `examples/scripts/`。

## SFTP 设计

SFTP 只在 SSH 会话可用时出现。

- 文件列表要清楚区分目录、普通文件和选中状态。
- 上传、下载和删除必须有明确进度或确认反馈。
- 新建、重命名和路径跳转应保留错误提示，避免静默失败。
- 拖放上传和右键菜单属于高风险交互，修改后需要手动验证。

## 内置文件服务器设计

文件服务按"配置与生命周期、协议适配器、共享目录安全、传输观测"分层。`FileServiceService` 只负责配置持久化、适配器选择和单一 runtime 生命周期；TFTP、FTP、SFTP adapter 各自封装协议细节，不直接读取前端状态，也不负责凭据持久化。

- TFTP 使用 Tokio 原生 UDP socket，按 RFC 1350/2347/2348/2349 实现独立传输 TID、ACK/DATA 重传和选项协商；应用层负责共享目录隔离、读写权限、文件 I/O 和传输进度观测。
- FTP 使用 `libunftp` + `unftp-sbe-fs`，由应用提供密码认证、主动/被动模式配置、被动端口范围和优雅 shutdown。
- SFTP 使用 `russh` + `russh-sftp`，支持 password auth、目录列表、stat、断点 offset 读写和持久化 host key。
- 三种协议都使用共享目录作为根目录；路径解析拒绝父目录跳转，FTP 文件系统后端额外提供 capability-based 根目录隔离。
- 密码只通过现有 credential store 的 `credential_id` 读取；普通服务配置只保存协议、端口、用户名和凭据引用。
- FTP 控制端口和被动数据端口由同一生命周期统一添加/删除防火墙规则；停止服务等待监听任务退出后再清理规则。
- `TransferRegistry` 统一记录三种协议的传输状态，并通过协议无关的 `file-transfer` 事件向前端发布。

### 变更历史

#### 2026-07-16 - 重写 TFTP 传输适配器

**变更内容**: 移除无法稳定处理跨平台临时 TID 的第三方 TFTP server，使用 Tokio 原生 UDP socket 重写 RFC 1350/2347/2348/2349 会话，补充协议解析、OACK 和重传测试。

**变更理由**: 第三方实现与 Tokio/Windows UDP 会话集成后无法可靠完成临时 TID 握手，导致跨主机上传下载超时。

**影响范围**: TFTP runtime、共享目录读写、文件传输进度事件和 Rust 依赖。

**决策依据**: TFTP 状态机规模有限且需要精确控制跨平台 socket 生命周期、临时 TID 和业务进度事件；自研实现可以直接验证每个 RFC 状态转换，并避免第三方 executor 与 Tokio 混用。

#### 2026-07-15 - 统一文件服务器与 FTP/SFTP 服务端重构

**变更内容**: 增加 FTP 服务端适配器，完善 SFTP 上传下载，加入统一协议选择、凭据引用、端口配置、被动端口范围、防火墙和 runtime shutdown 管理。

**变更理由**: 原有 TFTP 管理器只持有 TFTP/SFTP 两个专用 runtime，协议分派、配置和传输状态耦合在 TFTP 模块中，无法稳定扩展 FTP。

**影响范围**: Rust 文件服务生命周期、TFTP/SFTP runtime、FTP 服务端、凭据读取、防火墙、Tauri commands、工作区文件服务面板和本地化文案。

**决策依据**: FTP 控制/数据连接协议使用成熟的 `libunftp`，避免自行维护协议状态机；SFTP 继续复用项目已有 `russh` 栈；各协议只共享生命周期、配置和观测边界，不共享不兼容的传输协议细节。

## 设置页设计

设置页是工具配置面板，不承担教程功能。

- 子页面分组：通用、路径、外观、终端、关键字高亮、编辑器、传输、快捷键、关于（`src/views/settings/`）。
- 修改应尽量即时生效；需要持久化的偏好通过 Rust 或本地偏好状态保存。
- 路径迁移、日志级别、开发者工具、更新检查等涉及系统能力的操作，需要明确反馈。
- 恢复默认设置不能删除已保存连接和凭证。

## 国际化

界面文案位于 `src/i18n/locales/`。

- 新增用户可见文案时必须同时更新 `zh-CN.js` 和 `en-US.js`。
- 默认语义以中文为准，再补英文翻译。
- 避免在组件模板中写死用户可见字符串。

## 可访问性与交互反馈

- 图标按钮需要 `aria-label` 或可感知文本。
- 弹窗关闭、确认、取消状态要明确。
- 加载、保存、失败、空状态都应有 UI 表达。
- 焦点态使用已有 `--accent-glow`，不要移除键盘可见焦点。
- 禁用态要同时阻止交互并降低视觉权重。

## 修改前检查清单

- 是否复用了现有 token、shortcut、组件和 composable？
- 是否同时考虑亮色和暗色主题？
- 是否影响 `960 x 600` 最小窗口下的布局？
- 是否新增了用户可见文案，并同步 i18n？
- 是否涉及 Tauri 权限、文件系统、凭证或网络连接？
- 是否需要运行 `pnpm build`、`cargo check` 或手动启动 `pnpm tauri dev`？

## Tauri 插件接入约束

- 非 Windows 桌面端使用 `tauri-plugin-single-instance`，并保持 `deep-link` feature 开启；该插件必须在 `tauri-plugin-deep-link` 之前注册，第二实例回调只处理窗口可见性、聚焦等应用行为。
- Windows 桌面端继续保留自定义单实例实现。原因是原生 `tauri-plugin-single-instance` 在当前 Windows 运行环境存在兼容问题，不能作为该平台的可靠单实例入口。
- Windows 自定义实现必须使用命名 mutex 判断重复启动，并通过主窗口 HWND 上的私有 Win32 property 定位既有窗口；禁止使用窗口标题、进程名或模糊 class 名称查找主窗口。
- Windows 二次启动携带的 `ssh://` / `telnet://` 参数通过 `WM_COPYDATA` 发送给既有主窗口，再由首实例转发为 `deep-link://new-url` 事件。这是平台兼容桥，不属于可删除的冗余 deep-link 队列。
- `tauri-plugin-deep-link` 仍负责协议注册、启动参数识别和运行期 URL 事件分发。前端应直接使用插件 `getCurrent()` 与 `onOpenUrl()`，不要在应用层重复封装待处理队列或自定义去重协议。
- 后端只保留业务相关职责，例如把支持的 URI 解析为工作区可消费的临时连接对象。

## 变更历史

### 2026-07-14 - 终端会话生命周期与异步资源归属重构

**变更内容**: 重构终端 session activation、输出订阅交接、输出写入队列和后端 resize 防抖的生命周期管理。每次 activation 使用固定的 channel lease，旧订阅在 detach 完成并排空输出后才释放；输出写入使用 generation 取消旧会话的剩余写入；resize 请求携带 session 快照并拒绝跨会话同步。

**变更理由**: 重连、会话切换和组件卸载期间，异步输出、旧 channel 回调和防抖 resize 请求可能晚于资源切换完成，造成输出丢失、旧内容污染新终端或尺寸同步到错误会话。

**影响范围**: `src/utils/terminal/TerminalSessionRuntimeController.js`、`src/utils/terminal/addons/output/TerminalOutputAddon.js`、`src/utils/terminal/addons/resize/TerminalResizeAddon.js`、`src/components/TerminalPanel.vue`、终端后端自动认证输出顺序。

**决策依据**: 异步任务必须绑定创建时的 session/channel/generation；资源释放遵循"停止新输入、完成最后输出、解除订阅、清理本地状态"的顺序，避免依赖全局可变 channel 判断归属。

### 2026-06-15 - Deep Link 与单实例插件重构

**变更内容**: 删除自定义 `xterm://deep-link` 事件、Rust 侧 pending 队列和前端二次去重逻辑；前端改为直接调用 `@tauri-apps/plugin-deep-link` 的 `getCurrent()` / `onOpenUrl()`。非 Windows 单实例回调只保留窗口恢复职责；Windows 保留自定义 mutex、HWND 标记和 `WM_COPYDATA` 兼容桥。

**变更理由**: Tauri 2 的 `tauri-plugin-single-instance` 在启用 `deep-link` feature 后，可在非 Windows 平台把第二实例传入的 URL 交给 `tauri-plugin-deep-link` 处理。Windows 端因原生插件兼容问题保留自定义实现，但只保留必要的单实例唤醒和 URL 传递链路，不恢复应用级 pending 队列。

**影响范围**: `src-tauri/src/app.rs`、`src-tauri/src/deep_link.rs`、`src-tauri/src/state.rs`、`src/composables/useDeepLinkHandler.js`、`src/stores/workspaceExternalSessions.js`、项目说明文档。

**决策依据**: 深链接入职责按平台能力重新划分。非 Windows 依赖插件完成单实例参数分发；Windows 使用 Win32 原语完成可靠唤醒和转发。应用只消费支持的 `ssh://` / `telnet://` URL 并生成临时连接。

### 2026-07-02 - Windows 单实例窗口定位修正

**变更内容**: Windows 二次启动不再通过 `FindWindowW(NULL, "XTerm")` 按窗口标题定位既有窗口，改为首实例在主窗口 HWND 上注册私有 Win32 property，二次实例通过 `EnumWindows` 精确查找带该标记的窗口。

**变更理由**: 按标题查找会误命中其他标题为 `XTerm` 或 `xterm` 的终端窗口，导致二次启动参数被发送到错误窗口。私有 HWND property 是进程无关但窗口绑定的明确标记，能保留自定义 Windows 单实例兼容链路，同时避免误唤醒。

**影响范围**: `src-tauri/src/app.rs`、`docs/DESIGN.md`。

### 2026-05-17 - 跳板机编辑器与连接引用

**变更内容**: 将 SSH 跳板机配置从主连接弹窗中的密集字段块拆出为独立编辑器；`jumpHosts` 节点支持通过 `connectionId` 引用已有 SSH 连接，也继续兼容旧的手动主机节点。

**变更理由**: 跳板机链路是多步骤路由配置，不应占据主连接弹窗的基础字段区域；已有 SSH 连接本身已经封装主机、用户、凭证和自身跳板链路，复用连接比重复录入更符合运维工作流。

**影响范围**: 连接弹窗 SSH 表单、跳板机编辑器 UI、Workspace profile 序列化、SSH 跳板链路解析、连接引用循环校验、中英文文案。

**决策依据**: 新增 `connectionId` 仅保存在 `jumpHosts` JSON 中，不新增 SQLite 列；后端在连接时递归展开被引用 SSH 连接的跳板链路并检测引用环，前端主弹窗只展示摘要和配置入口。

### 2026-05-17 - SSH 多级跳板机链路

**变更内容**: SSH 连接配置从单个跳板机字段扩展为有序 `jumpHosts` 链路，保留旧单跳字段兼容读取；后端按顺序建立 direct-tcpip 隧道并在会话结束时清理所有上游 SSH 句柄。

**变更理由**: 真实内网维护场景经常需要多级堡垒机链路，单跳配置无法表达目标主机的完整可达路径。

**影响范围**: 连接配置存储、SQLite schema migration、Workspace profile 序列化、SSH 连接建立与主机公钥确认流程、连接弹窗 SSH 表单与中英文文案。

**决策依据**: 多跳链路作为 SSH 配置的一部分持久化为 JSON 数组，避免为每一级跳板机扩展固定列；旧单跳字段继续用于兼容历史数据。

### 2026-05-09 - 视图布局与样式体系破坏性重构

**变更内容**: 重建全局样式层级、OKLCH token、最小窗口布局基线、logical size 填充工具、Anchor Positioning 回退和 View Transition 路由过渡；移除媒体查询、JS 色彩媒体查询和百分比宽度路径。

**变更理由**: 让桌面工具界面以最小窗口为稳定基准，通过 Flex/Grid、Container Queries、`clamp()` 和原生新 CSS 能力承担适配，减少手写响应式分支。

**影响范围**: `index.html`、`src/styles.scss`、`uno.config.js`、路由、主题偏好、终端/SFTP/连接弹窗/确认弹窗/设置与侧栏相关组件。

**决策依据**: 破坏旧版兼容，优先统一布局和样式约束；旧的媒体查询、百分比宽度、JS 系统主题监听和进度条宽度计算均被视为可删除实现。
