# AGENTS.md

本文件面向 AI 编码 Agent，介绍本仓库的架构、命令与约定。读者默认对项目一无所知。

## 项目概述

XTerm 是一个 Tauri 2 桌面终端工作区应用：在一个本地客户端中整合 SSH / Telnet / 串口连接、交互式终端（XTerm.js）、SFTP 文件管理、内置 TFTP/FTP/SFTP 文件服务器、凭证管理、会话记录和设置。

- 前端：Vue 3（`<script setup>`）+ Vite + Pinia + vue-router + vue-i18n（中英文）+ UnoCSS + SCSS + XTerm.js 6 + CodeMirror 6。
- 后端：Rust + Tauri 2，tokio 异步运行时。SSH/SFTP 用 `russh` / `russh-sftp`，FTP 服务端用 `libunftp`，串口、本地存储（`redb`）、keyring 加密凭证、防火墙管理等均为原生实现。
- 包管理：`pnpm`（锁文件 `pnpm-lock.yaml`）。
- 应用标识：`com.liushicong.xterm`，版本 `0.1.1`，打包目标为 NSIS（`pnpm tauri build`）。

## 项目结构

- `index.html`：Vite HTML 入口；`src/main.js` 创建 Vue 应用并注册 i18n、router、UnoCSS、全局 SCSS；`src/App.vue` 挂载应用壳层。
- `src/views/`：页面（会话、工作区、凭证、密钥、脚本、Dashboard、设置）。
- `src/components/`：终端、SFTP、连接弹窗、通知、选择器等组件。
- `src/composables/`：可复用逻辑（工作区状态、偏好设置、SFTP 操作、终端运行时等，均以 `use*` 命名）。
- `src/stores/`：Pinia store（连接状态机、工作区会话、终端几何、主机公钥提示、用户脚本等）。
- `src/services/`：前端到 Tauri Rust 命令的封装（`ipc/` 子目录为底层调用；`scripting/` 子目录为脚本引擎：终端桥接、运行器、弹窗交互）。组件中不要散落裸 `invoke`，应在 service 中添加明确封装。
- `src/i18n/`：中英文案。`src/utils/`：通用工具（如 `eventBridge.js`）。
- `src-tauri/src/`：Rust 后端。`lib.rs` 初始化状态、插件并注册 `tauri::generate_handler!`；模块包括 `terminal/`（api/app/domain/protocol 分层）、`credentials/`、`file_service/`、`tftp/`、`storage/`、`session_recording/`、`paths/`、`logging/`、`proxy/`、`firewall.rs` 等。
- 日志体系：`src-tauri/src/logging/`（`level`/`event`/`writer`/`retention`/`panic`/`commands` 子模块）。后端写日志一律用 `crate::logging::event(scope, action)` 结构化事件或带显式 `target: "<scope>"` 的 `log::<level>!`，scope 为点分逻辑名（如 `terminal.serial`），不使用默认模块路径 target；panic/启动应急路径除外。日志按日写入 `<log_dir>/YYYYMMDD.log`（无缓冲逐条 flush，保留 7 天 / 最多 14 个文件），`panic.log`、`startup-error.log` 超 4 MiB 截尾。级别持久化于 settings（`logLevel`），`log_level_set` 立即生效；嘈杂依赖 crate（russh/keyring/mio/tao）在级别低于 debug 时被钳制。前端统一用 `createLogger("frontend.<area>.<module>")` + 点分事件名（`src/utils/logger.js`），启动时经 `src/services/logging.js` 同步后端级别；生产模式 error/warn 转发到后端日志文件。日志相关 Tauri 命令：`log_level_get/set`、`log_files_list`、`log_file_tail`、`log_files_prune`、`log_dir_open`；设置页"通用"内置日志查看对话框。
- `src-tauri/capabilities/default.json`：Tauri 2 权限能力配置。
- `src-tauri/tauri.conf.json`：开发地址（`http://127.0.0.1:1420`）、`dist` 输出、无边框窗口（`decorations: false`）、deep-link scheme（`ssh`、`telnet`）和打包配置。
- `tests/`：Node 内置测试运行器的前端单元测试（如 `eventBridge.test.js`、`connectionStateMachine.test.js`）。
- `docs/DESIGN.md`、`docs/SCRIPTING.md`、`docs/HIGHLIGHTING.md`、`README.md`：设计、脚本编写、关键字高亮使用指南与产品说明（中文）。
- `examples/`：可供用户导入的示例资源（`highlight-schemes/` 终端高亮方案、`scripts/` 示例脚本，kebab-case 命名）。

不要直接编辑 `node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/` 等生成或依赖目录。

## 构建、测试与开发命令

- `pnpm install`：安装前端依赖和 Tauri CLI。
- `pnpm dev`：启动 Vite 开发服务（`127.0.0.1:1420`，strictPort）。
- `pnpm build`：构建前端到 `dist/`（terser 压缩，按包手动分包）。
- `pnpm test`：运行前端单元测试（`node --test`，无独立测试框架）。
- `pnpm lint`：ESLint（`--max-warnings=0`）+ Stylelint + `cargo clippy -- -D warnings`。
- `pnpm format` / `pnpm format:check`：Prettier（js/scss）+ ESLint/Stylelint fix + `cargo fmt`。
- `pnpm check`：`format:check` + `lint` + `test` 一键检查。
- `pnpm tauri dev`：启动 Tauri 桌面开发环境（自动执行 `pnpm dev`）。
- `pnpm release`（即 `tauri build`）/ `pnpm release:debug`：构建生产/调试桌面包。
- `cd src-tauri && cargo check`：快速检查 Rust 后端。

修改 Rust 后端后至少运行 `cargo check`（理想情况 `cargo clippy` 无警告）；修改前端后至少运行 `pnpm build` 和 `pnpm test`。涉及界面、窗口、主题、SFTP、终端或权限的变更，需要通过 `pnpm tauri dev` 手动验证。

## 编码风格

- JavaScript、Vue、SCSS、JSON 使用两空格缩进，Prettier 格式化；Rust 使用 `rustfmt`。
- Vue 组件统一 `<script setup>`；JS 变量/函数用 `camelCase`；Rust 用 `snake_case`；CSS 类名和资源文件名用描述性 kebab-case。
- ESLint 未使用变量以 `_` 前缀豁免（`argsIgnorePattern: "^_"` 等）。
- Rust 命令保持小而清晰，用 `#[tauri::command]` 并在 `src-tauri/src/lib.rs` 注册；改命令时同步检查 `src/services/` 调用点。
- 优先延续现有组件、composable、service、UI token、UnoCSS shortcut 和 SCSS 变量，不为局部改动引入新框架、新依赖或大抽象。
- 做最小可用变更，不把功能、重构、视觉重做和配置改动混在一起。

## 实现边界

- UI 状态和交互留在 `src/`；原生能力、网络连接、文件系统、加密、日志、SQLite/redb 和系统集成留在 `src-tauri/`。
- 连接、凭证、路径、日志、会话记录等跨边界功能，需要同时维护前端 service、Rust 命令和存储行为。
- 终端附加能力（搜索、剪贴板 OSC 52、超链接、Unicode 11、连字、进度序列）默认通过 xterm addon 加载；WebGL 渲染器有独立开关。OSC 1337 CurrentDir 用于 SFTP 跟随 shell 目录（应用不自动注入 shell 集成脚本）。

## 安全与权限

- Tauri 权限（`capabilities/default.json`）保持最小化；新增 native 能力前确认已有命令或插件能否满足。不要无理由放宽 CSP、窗口能力、文件系统或外部打开权限。
- 凭证列表接口不得返回密码、私钥或私钥密码；敏感内容由本地后端（keyring）加密保存。
- SSH 主机公钥需用户确认后才信任；首次连接展示指纹。
- 涉及本地路径、注册表、keyring、数据库和日志迁移的变更，应把失败模式处理清楚。
- FTP/SFTP 服务器密码保存在 OS keyring（service `com.liushicong.xterm`，account `file-service-password`），不出现在配置快照中（快照只暴露 `passwordSet`）；SFTP host key 保存在应用数据目录以稳定设备指纹。

## 文档原则

代码是主要文档。除非变更影响开发者安装、运行、权限、安全模型、打包发布或外部集成，否则不要新增独立 Markdown 文件（禁止 `SUMMARY.md`、`WORK_REPORT.md` 之类的重复说明）。注释应说明"为什么这样做"和"失败时会怎样"，而不是复述代码。

## 常见反模式

- 未读实现就修改 Tauri 命令、capability、前端交互或文档。
- 用旧文档覆盖当前 Vue/Vite/Tauri 结构。
- 为单个功能引入额外层、全局状态或新依赖。
- 新增宽泛权限只为绕过局部问题。
- 把用户未要求的视觉重做、重构和功能改动混入同一提交。
