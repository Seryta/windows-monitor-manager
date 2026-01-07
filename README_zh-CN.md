# Windows Monitor Manager Service

**中文** | [English](README.md)

一个基于 Rust 编写的强大 Windows 服务程序，提供 RESTful API 和 Web 界面，用于远程管理物理显示器和显示拓扑。

它专为解决 Windows 服务（Session 0）和**登录界面（WinLogon）**下无法调用显示 API 的痛点而设计，即使在未登录状态下也能稳定工作。

## ✨ 功能特性

*   **Session 0 隔离穿透**：采用高级令牌窃取和进程快照技术（类似 RustDesk 原理），将代理程序注入到当前的活动控制台会话（用户桌面或登录界面）。
*   **支持登录界面**：即使电脑刚开机未登录，也能获取屏幕信息并切换显示模式。
*   **Web 管理界面**：内置现代化的 Web UI，方便通过手机或笔记本远程管理。
*   **单文件架构**：所有组件（服务、API、代理）整合在单个 `monitor-service.exe` 文件中，部署极其简单。
*   **可靠的显示控制**：使用 Windows **CCD** (Connecting and Configuring Displays) API 替代过时的 GDI，确保显示器识别和拓扑切换的准确性。
*   **智能过滤**：自动隐藏虚假的 "Generic Non-PnP Monitors" 和非活动显示器，只展示真实物理屏幕。
*   **独占激活模式**：支持“仅显示在 X 屏幕”功能，底层严格禁用其他所有显示路径。

## 🚀 快速开始

### 1. 控制台模式 (测试用)
直接在终端运行以测试功能，无需安装服务。

```powershell
.\monitor-service.exe --addr 0.0.0.0:3000
```
然后打开浏览器访问：`http://localhost:3000`

### 2. 安装为 Windows 服务
如果您希望它开机自启（并支持在登录界面工作）：

1.  以**管理员身份**打开 PowerShell。
2.  创建服务（请修改为您的实际路径）：
    ```powershell
    sc.exe create MonitorService binPath= "C:\path\to\monitor-service.exe --service --addr 0.0.0.0:3000" start= auto
    ```
    > **注意**：`binPath=` 等号后面必须有一个空格。
3.  启动服务：
    ```powershell
    sc.exe start MonitorService
    ```

### 3. 停止/卸载
```powershell
sc.exe stop MonitorService
sc.exe delete MonitorService
```

## 🛠️ 使用说明

### Web 界面
访问 `http://YOUR_PC_IP:3000` 可以：
*   查看当前活动的物理显示器列表。
*   **点击显示器卡片**：直接切换到“仅该屏幕显示”模式。
*   **快捷操作**：一键切换 **Internal** (仅电脑), **External** (仅第二屏), **Extend** (扩展), **Duplicate** (复制)。

### 命令行参数
```text
Usage: monitor-service.exe [OPTIONS] [COMMAND]

Options:
  --service             以 Windows 服务模式运行
  --addr <ADDR>         监听地址和端口 [默认: 0.0.0.0:3000]
  -h, --help            打印帮助信息

Commands:
  agent                 作为代理运行 (内部使用，用于会话注入)
```

## 🏗️ 构建指南

环境要求：`rustup`, `cargo` (在 Windows 环境下)。

1.  克隆仓库。
2.  编译 Release 版本：
    ```bash
    cargo build --release
    ```
3.  生成的可执行文件位于 `target/release/monitor-service.exe`。

## 🧩 架构原理

*   **Service 进程 (Session 0)**: 运行 Axum Web 服务器，处理 API 请求。它负责检测当前活动的控制台 Session ID。
*   **Agent 进程 (User Session)**: 服务进程使用 `CreateProcessAsUserW` 将自身 (`monitor-service.exe agent`) 启动到用户会话（或 WinLogon 会话）中。
*   **通信机制**: 服务进程通过捕获 Agent 子进程的标准输出 (`stdout`) 来获取显示器列表和执行结果。

## ⚠️ 常见问题

*   **"No monitors found" (未找到显示器)**: 请确保服务正在运行，且您不是通过远程桌面 (RDP) 连接的（RDP 使用虚拟显示驱动）。本工具管理的是**物理**显卡输出。
*   **显示器重复**: 工具会根据硬件 Target ID 自动去重。如果您仍然看到重复项，可能是驱动残留的幽灵条目，尝试重启电脑通常可以解决。

## 致谢

*   感谢 [RustDesk](https://github.com/rustdesk/rustdesk)。

