# 仓库协作指南

## AI Agent 工作要求

### 修改前必须理解现有实现

在修改任何代码或文档前，先检查相关实现、调用点和配置。不要只根据文件名或旧文档推断项目状态。

推荐流程：

1. **定位模块**
   - 前端应用：`src/`
   - 前端入口：`index.html`、`src/main.js`、`src/App.vue`
   - 路由与页面：`src/router/`、`src/views/`
   - 组件与状态：`src/components/`、`src/composables/`
   - Tauri 后端：`src-tauri/src/`
   - 权限能力：`src-tauri/capabilities/`
   - 应用配置：`src-tauri/tauri.conf.json`

2. **读取相关文件**
   - 改前端交互时，同时检查组件、composable、service 和路由状态。
   - 改 Tauri 命令时，同时检查 Rust 命令定义、`src/services/` 调用点和 `tauri::generate_handler!` 注册。
   - 改权限、窗口、路径或打包行为时，同时检查 capability 文件和 `tauri.conf.json`。

3. **记录本地模式**
   - Vue 组件的状态组织方式。
   - `invoke` 命令封装方式。
   - Rust 命令的输入输出和错误返回格式。
   - 现有 UI token、UnoCSS shortcut 和 SCSS 变量。

## 项目结构

本仓库是一个 Tauri 2 桌面应用，前端已经迁移为 Vue 3 + Vite，而不是纯静态 HTML/CSS/JavaScript。

- `index.html` 是 Vite HTML 入口。
- `src/main.js` 创建 Vue 应用，并注册 `vue-i18n`、`vue-router`、UnoCSS 和全局 SCSS。
- `src/App.vue` 挂载应用壳层、TooltipProvider 和 ToastProvider。
- `src/layouts/` 保存应用壳层和桌面窗口布局。
- `src/views/` 保存会话、工作区、凭证、设置等页面。
- `src/components/` 保存终端、SFTP、连接弹窗、通知、选择器等组件。
- `src/composables/` 保存工作区状态、偏好设置和通知逻辑。
- `src/services/` 封装前端到 Tauri Rust 命令的调用。
- `src/i18n/` 保存中英文界面文案。
- `src-tauri/src/` 保存 Rust 后端模块。`lib.rs` 负责初始化状态、插件和命令注册。
- `src-tauri/capabilities/` 保存 Tauri 2 权限能力配置。
- `src-tauri/tauri.conf.json` 定义 Vite 开发地址、`dist` 输出、窗口和打包配置。

不要直接编辑 `node_modules/`、`dist/`、`src-tauri/target/` 等生成或依赖目录。

## 构建、测试与开发命令

使用 `pnpm` 管理 JavaScript 依赖，锁文件为 `pnpm-lock.yaml`。

- `pnpm install`：安装前端依赖和 Tauri CLI。
- `pnpm dev`：启动 Vite 开发服务，默认监听 `127.0.0.1:1420`。
- `pnpm build`：构建前端到 `dist`。
- `pnpm preview`：预览前端构建产物。
- `pnpm tauri dev`：启动 Tauri 桌面开发环境，并自动执行 `pnpm dev`。
- `pnpm tauri build`：构建生产桌面包，并自动执行 `pnpm build`。
- `cd src-tauri && cargo check`：检查 Rust 后端。
- `cd src-tauri && cargo fmt`：格式化 Rust 代码。

当前没有单独配置前端测试框架。修改 Rust 后端后至少运行 `cargo check`；修改前端后至少运行 `pnpm build`。涉及界面、窗口、主题、SFTP、终端或权限的变更，需要通过 `pnpm tauri dev` 手动验证。

## 编码风格

- JavaScript、Vue、SCSS、HTML、JSON 使用两空格缩进。
- Rust 使用 `rustfmt`。
- Vue 组件采用 `<script setup>`。
- JavaScript 变量和函数使用 `camelCase`。
- Rust 函数、字段和模块使用 `snake_case`。
- CSS 类名和资源文件名使用描述性 kebab-case。
- 前端调用 Rust 命令时，优先在 `src/services/` 中添加明确封装，不要在组件中散落裸 `invoke`。
- Rust 命令保持小而清晰，使用 `#[tauri::command]` 并在 `src-tauri/src/lib.rs` 注册。

## 实现边界

- UI 状态和交互留在 `src/`。
- 原生能力、网络连接、文件系统、加密、日志、SQLite 和系统集成留在 `src-tauri/`。
- 连接、凭证、路径、日志、会话记录等跨边界功能，需要同时维护前端 service、Rust 命令和存储行为。
- 优先延续现有组件、composable、service 和 SCSS token，不为局部改动引入新框架或大抽象。
- 做最小可用变更，避免把功能、重构、视觉重做和配置改动混在一起。

## 文档原则

代码是主要文档。除非变更影响开发者安装、运行、权限、安全模型、打包发布或外部集成，否则不要新增独立 Markdown 文件。

适合写注释的地方：

- 非显然的 Rust 命令、平台差异、权限或安全约束。
- 文件系统、终端连接、会话记录、凭证加密、路径迁移等有副作用的逻辑。
- 复杂前端状态同步、终端尺寸同步、SFTP 拖放或跨组件数据流。

避免只复述代码的注释。说明“为什么这样做”和“失败时会怎样”比说明“这一行在赋值”更有价值。

## 安全与权限

- Tauri 权限配置应保持最小化。
- 新增 native 能力前先确认是否已有命令或插件能满足需求。
- 不要无理由放宽 CSP、窗口能力、文件系统访问或外部打开权限。
- 凭证列表接口不得返回密码、私钥或私钥密码。
- 内置 FTP/SFTP 文件服务器密码保存在 OS keyring（service `com.liushicong.xterm`，account `file-service-password`）；对前端的配置快照只暴露 `passwordSet`，绝不输出密码本身。
- 涉及本地路径、注册表、keyring、数据库和日志迁移的变更，应把失败模式处理清楚。

## 常见反模式

- 未读实现就修改 Tauri 命令、capability、前端交互或文档。
- 用旧文档覆盖当前 Vue/Vite/Tauri 结构。
- 为单个功能引入额外层、全局状态或新依赖。
- 新增宽泛权限只为绕过局部问题。
- 创建 `SUMMARY.md`、`WORK_REPORT.md`、`IMPLEMENTATION_NOTES.md` 等重复说明文件。
- 把用户未要求的视觉重做、重构和功能改动混入同一提交。
