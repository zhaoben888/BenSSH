# ⚡ BenSHH (Rust Edition)

> 一个极客级别的 VPS 集群控制中枢与分布式文件漫游引擎。完全由 Rust 打造。

![Rust](https://img.shields.io/badge/Language-Rust-orange.svg)
![Ratatui](https://img.shields.io/badge/UI-Ratatui-blue.svg)
![Zero_Dependency](https://img.shields.io/badge/Dependencies-Zero-brightgreen.svg)

## 🔥 核心特性 (Features)

- 🚀 **毫秒级 TUI 引擎**：基于 `Ratatui` 渲染的纯终端图形界面，双模状态机（左右分屏）。
- 🖥️ **无缝多标签 PTY**：打破 TUI 限制，一键呼出 Windows Terminal 新标签页进行 PTY 会话。
- 📂 **SFTP 极速文件流**：直接在终端浏览远程目录。
- 🛸 **GUI 降维打击**：在纯命令行中，利用 `rfd` 穿透调起 Windows 原生弹窗进行文件上传与下载，原生支持同名覆盖警告。
- 🔑 **核弹级免密注入**：一键后台生成 RSA 2048 私钥，强行破门注入远程 Linux 服务器 `authorized_keys`，并自动抹除本地明文密码。
- 📦 **零依赖跨平台**：纯二进制文件发布，即开即用，告别 Node.js 环境。

## 🛠️ 构建与安装 (Build & Install)

本程序支持一键自动化打包与部署。

```bash
# 1. 开启最强优化编译
cargo build --release

# 2. 将编译出的 target/release/benshh-rs.exe 改名为 benshh.exe
# 3. 将其与项目根目录的 install.ps1 放在一起，右键使用 PowerShell 运行 install.ps1 即可一键注入环境变量和生成桌面图标。
```

## ⌨️ 快捷键映射 (Keybindings)

### 服务器列表模式 (左侧)
- `Enter` : 在新标签页连接当前高亮的服务器
- `f`     : 打开远端文件管理面板 (SFTP)
- `a`     : 添加新节点
- `e`     : 编辑当前节点
- `x`     : 永久删除该节点
- `s`     : 执行底层证书免密配置（自动生成公钥并下发）
- `q`     : 退出程序

### 文件漫游模式 (右侧)
- `Enter` : 进入选中目录 / 返回上一级 (`..`)
- `d`     : 调用 Windows 原生框，下载该文件到本地
- `u`     : 调用 Windows 原生框，将本地文件上传到当前远端目录
- `b`     : 退出文件面板，返回左侧列表
- `q`     : 退出程序
