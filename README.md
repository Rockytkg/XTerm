# XTerm

XTerm 是一款 Tauri 2 桌面终端工作区应用，把 SSH / Telnet / 串口连接、交互式终端、SFTP 文件管理、内置文件服务器、凭证管理和会话记录整合在一个本地客户端中，适合需要频繁维护服务器、网络设备和串口设备的场景。

## 功能特性

- **多协议连接**：SSH（主机公钥确认、多级跳板机链路）、Telnet、串口（自动检测波特率），连接配置可保存复用。
- **交互式终端**：基于 XTerm.js，支持搜索、滚动缓冲、OSC 52 剪贴板、超链接、Unicode 11、连字、进度序列，可选 WebGL 渲染器；连接失败、认证失败等状态在界面中明确反馈。
- **SFTP 文件管理**：SSH 会话内置文件面板，支持浏览、上传、下载、新建、重命名、删除，传输进度可见；可通过 `OSC 1337 CurrentDir` 跟随 shell 当前目录。
- **内置文件服务器**：TFTP / FTP / SFTP 服务器，用于向交换机等网络设备提供升级文件，支持防火墙生命周期管理。
- **凭证管理**：密码与 SSH 私钥由本地后端（OS keyring）加密保存，列表接口不回传敏感内容。
- **终端高亮**：可配置的正则/关键字高亮方案，支持导入导出（示例见 `examples/highlight-schemes/`）。
- **脚本引擎**：油猴风格的 JavaScript 终端自动化脚本，运行在独立 Web Worker 沙盒中，支持交互表单、文件读写授权和会话记录控制（见 [脚本编写指南](docs/SCRIPTING.md)）。
- **会话辅助**：会话记录、运行时状态概览、日志级别设置、终端编码检测、主题与快捷键偏好，中英文界面，亮/暗/跟随系统主题。

## 技术栈

- **前端**：Vue 3（`<script setup>`）+ Vite + Pinia + vue-router + vue-i18n + UnoCSS + SCSS + XTerm.js 6 + CodeMirror 6
- **后端**：Rust + Tauri 2（tokio 异步运行时），SSH/SFTP 基于 `russh` / `russh-sftp`，FTP 服务端基于 `libunftp`，本地存储使用 `redb`
- **包管理**：pnpm

## 快速开始

环境要求：Node.js、pnpm、Rust 工具链，以及 Tauri 2 的[系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install        # 安装依赖
pnpm tauri dev      # 启动桌面开发环境
```

常用命令：

```bash
pnpm dev            # 仅启动 Vite 前端开发服务（127.0.0.1:1420）
pnpm build          # 构建前端到 dist/
pnpm test           # 运行前端单元测试（node --test）
pnpm lint           # ESLint + Stylelint + cargo clippy
pnpm check          # format:check + lint + test 一键检查
pnpm release        # 构建生产桌面包（NSIS）
```

## Linux 平台说明

### 运行依赖

Linux 上应用基于 WebKitGTK 渲染，运行前需要安装以下系统包（Debian/Ubuntu 名称）：

```bash
sudo apt install libwebkit2gtk-4.1-0 libayatana-appindicator3-1
```

凭证（密码、私钥）经 OS keyring 加密保存，Linux 上依赖 Secret Service 实现——桌面环境通常自带（GNOME 的 gnome-keyring、KDE 的 KWallet）。无头/最小化环境若提示 keyring 相关错误，请先安装并解锁 gnome-keyring。

### 串口访问权限（重要）

Linux 的串口设备节点（`/dev/ttyUSB*`、`/dev/ttyACM*` 等）默认归属 `root` 和设备组（Debian/Ubuntu/Fedora 为 `dialout`，Arch 为 `uucp`），普通用户默认无权读写，打开串口会报“没有权限访问”。参考 Wireshark 的做法，把当前用户加入对应组即可（无需 root 运行应用）：

```bash
# Debian / Ubuntu / Fedora
sudo usermod -aG dialout $USER

# Arch Linux
sudo usermod -aG uucp $USER
```

**重新登录**（或重启）后生效。可用 `id -nG` 检查组是否已包含 `dialout`/`uucp`，用 `ls -l /dev/ttyUSB0` 确认设备节点所属组。

应用侧的配合：打开串口被内核拒绝（EACCES）时，连接错误会直接提示上述用户组操作，而不是笼统的“端口不可用”。

### Wayland 会话

右键菜单始终在主窗口内以 DOM 渲染，不依赖全局光标坐标或窗口绝对定位（Wayland 出于安全设计禁止客户端使用这两类能力），因此 Wayland 与 X11 下行为一致，无需额外配置。

### NVIDIA 闭源驱动渲染异常

WebKitGTK 在 NVIDIA 专有驱动下可能出现页面空白或严重掉帧（DMA-BUF 渲染器兼容问题）。如遇此情况，用环境变量禁用该渲染器后启动：

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 xterm
```

### 构建 Linux 安装包

仓库默认的打包目标是 Windows NSIS；构建 Linux 包（deb / rpm / AppImage）需在 Linux 环境执行：

```bash
pnpm tauri build -- --bundles deb    # 或 rpm / appimage
```

## 项目结构

```
src/          前端源码（views / components / composables / stores / services / i18n / utils）
src-tauri/    Rust 后端（terminal / credentials / file_service / tftp / storage / logging 等模块）
tests/        前端单元测试（Node 内置测试运行器）
docs/         设计文档与脚本编写指南
examples/     可导入的示例资源（终端高亮方案、示例脚本）
```

## SFTP 跟随 shell 当前目录

应用不会向 SSH 会话自动注入 shell 集成脚本。如需 SFTP 面板跟随 shell 当前目录，让远端 shell 主动输出 `OSC 1337;CurrentDir=...` 序列：

```bash
# Bash（追加到 ~/.bash_profile）
export PS1="$PS1\[\e]1337;CurrentDir="'$(pwd)\a\]'

# Zsh（追加到 ~/.zshrc）
precmd () { echo -n "\x1b]1337;CurrentDir=$(pwd)\x07" }

# Fish（追加到 ~/.config/fish/config.fish）
function __tabby_working_directory_reporting --on-event fish_prompt
    echo -en "\e]1337;CurrentDir=$PWD\x7"
end
```

重新登录后，SFTP 面板会自动跟随当前工作目录。

## 文档

- [docs/DESIGN.md](docs/DESIGN.md)：设计文档
- [docs/SCRIPTING.md](docs/SCRIPTING.md)：脚本编写指南
- [docs/HIGHLIGHTING.md](docs/HIGHLIGHTING.md)：终端关键字高亮使用指南
- [AGENTS.md](AGENTS.md)：仓库架构、命令与约定（面向 AI 编码 Agent）

## 安全说明

XTerm 是本地桌面应用：连接配置和偏好设置保存在本地数据库，密码、私钥和私钥密码由 OS keyring 加密保存，SSH 主机公钥需用户确认后才会被信任。Tauri 权限能力保持最小化。

## 问题反馈

如遇 Bug 或有功能建议，欢迎在 [GitHub Issues](https://github.com/Rockytkg/XTerm/issues) 反馈。请尽量提供以下信息，以便快速定位：

- 复现步骤与预期/实际行为
- 操作系统版本与应用版本（`设置 → 关于`）
- 相关日志：打开 `设置 → 通用` → 日志查看器，或直接查看应用日志目录

## 许可证

XTerm 以 [MIT License](LICENSE) 开源，Copyright © 2026 Rockytkg。使用者可自由使用、修改、分发与商用，仅需保留版权声明，详见 [LICENSE](LICENSE) 文件。
