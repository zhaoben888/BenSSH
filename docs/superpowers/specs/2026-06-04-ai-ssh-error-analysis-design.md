# AI SSH Error Analysis Design

## 背景

BenSSH 当前有两条主要运维路径：

- `Enter` 通过 Windows Terminal 打开原生 `ssh` 会话。
- `f` 进入内置 SFTP 文件面板，错误通过 `feedback_msg` 弹窗展示。

Windows Terminal 会话由外部 `ssh` 进程接管，主程序无法稳定读取远端命令输出，因此不适合作为 AI 自动诊断的数据源。新增的 AI SSH 登录功能应由 Rust 程序直接建立 SSH 会话，让 TUI 能捕获终端输出，并在右侧自动给出错误解释和解决方案。

## 目标

- 保留原有 Windows Terminal 登录方式。
- 新增 AI SSH 登录方式，适合 AI 自动观察输出。
- 当远端输出出现常见错误时，右侧 AI 面板自动显示中文解释和建议。
- 优先提供本地规则分析，保证离线可用。
- 支持 DeepSeek，同时预留其他 OpenAI-compatible 大模型接入。
- 避免发送密码、私钥路径等敏感信息给模型。

## 非目标

- 不在第一版实现完整终端模拟器能力，例如 `vim`、`top`、全屏 ncurses 程序的完美渲染。
- 不替换原有 Windows Terminal SSH 登录。
- 不自动执行 AI 建议中的修复命令，所有修复仍由用户确认并手动输入。

## 用户交互

服务器列表模式新增快捷键：

- `Enter`：保持现状，打开 Windows Terminal 原生 SSH。
- `i`：进入 AI SSH 模式。

AI SSH 模式布局：

- 左侧主区域：显示远端 shell 输出，并接收用户键盘输入。
- 右侧窄面板：显示 AI 诊断结果，包括错误解释、可能原因、建议命令和风险提醒。
- 底部帮助栏：显示 `Esc/b` 返回、普通按键发送到远端、`Ctrl+C` 中断远端命令等提示。

AI SSH 模式退出：

- `Esc` 或 `b` 返回服务器列表。
- 退出时关闭 SSH channel 和后台读取任务。

## 架构

新增模块：

- `src/ai.rs`
  - 定义 `AiConfig`、`AiAnalysis`。
  - 提供本地规则分析函数。
  - 提供 OpenAI-compatible chat/completions HTTP 调用。
  - 支持 DeepSeek 默认配置和通用模型配置。

- `src/ai_ssh.rs`
  - 负责建立 `ssh2::Session`。
  - 请求 PTY 并启动远端 shell。
  - 将键盘输入写入远端 channel。
  - 后台读取远端输出，维护最近输出缓冲。

- `src/main.rs`
  - 新增 `AppMode::AiSsh`。
  - 管理 AI SSH 状态、输出滚动缓冲、右侧诊断文本。
  - 在输出命中错误模式时触发分析。

依赖变化：

- 使用 `reqwest` 调用 DeepSeek/OpenAI-compatible API。
- 使用 `serde_json` 组装请求和解析响应，仓库已存在该依赖。

## 模型配置

环境变量优先级：

- API Key：优先 `DEEPSEEK_API_KEY`，其次 `OPENAI_API_KEY`。
- Base URL：优先 `DEEPSEEK_BASE_URL`，其次 `OPENAI_BASE_URL`。
- Model：优先 `DEEPSEEK_MODEL`，其次 `OPENAI_MODEL`。

默认值：

- DeepSeek base URL：`https://api.deepseek.com`
- DeepSeek model：`deepseek-chat`
- OpenAI-compatible endpoint：`/chat/completions`

如果没有 API Key：

- 不报错。
- 右侧面板显示本地规则诊断，并提示“未配置 API Key，当前为离线分析”。

## 错误检测

第一版使用关键词和上下文窗口触发：

- `error`
- `failed`
- `permission denied`
- `no such file or directory`
- `command not found`
- `connection refused`
- `timeout`
- `denied`
- `cannot`
- `unable`
- `segmentation fault`
- `out of memory`

触发策略：

- 只分析最近输出窗口，例如最近 80 行或最近 8 KB。
- 同一错误上下文短时间内不重复分析。
- 默认节流 5 秒，避免用户连续输出时报文刷屏。

## 本地规则分析

本地规则覆盖高频运维错误：

- 权限不足：解释文件权限、用户身份、`sudo`、目录拥有者。
- 文件不存在：提示检查路径、当前目录、拼写、软链接。
- 命令不存在：提示安装包、检查 PATH、确认发行版包名。
- 连接拒绝：提示服务是否监听、防火墙、安全组、端口。
- 超时：提示网络连通性、防火墙、安全组、DNS。
- 磁盘空间不足：提示 `df -h`、清理日志、定位大文件。
- 内存不足：提示 `free -h`、`dmesg`、服务资源限制。

本地规则输出格式与 AI 输出一致，便于 UI 统一渲染。

## AI 提示词

发送给模型的内容包括：

- 当前操作系统上下文：这是一个 SSH 到 Linux 主机的运维工具。
- 服务器非敏感信息：节点别名、登录用户、主机地址可脱敏或只给别名。
- 最近终端输出：只截取最近错误上下文。
- 输出要求：中文、简洁、分为“含义”“可能原因”“建议步骤”“风险提醒”。

不会发送：

- 密码。
- 私钥内容。
- 私钥路径。
- 本地配置文件完整内容。

## 错误处理

- SSH 连接失败：仍使用现有弹窗或 AI 面板给出本地规则解释。
- AI API 请求失败：右侧保留本地分析，并显示 API 失败原因摘要。
- JSON 解析失败：显示“模型返回格式异常”，保留原始本地分析。
- 网络不可用：不影响 AI SSH 基础登录和本地规则分析。

## 测试与验证

最低验证：

- `cargo check`
- `cargo test`，如果仓库后续添加测试。
- 手动启动程序，确认：
  - `Enter` 仍打开 Windows Terminal SSH。
  - `i` 能进入 AI SSH 模式。
  - 常见错误如 `ls /not-exist` 能触发右侧建议。
  - 未配置 API Key 时离线诊断可用。
  - 配置 DeepSeek API Key 时能显示模型诊断。

## README 更新

README 需要新增：

- `i` AI SSH 登录快捷键。
- DeepSeek 环境变量示例。
- OpenAI-compatible 接口说明。
- 隐私说明：只发送最近错误上下文，不发送密码和私钥。
