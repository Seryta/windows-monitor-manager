# Windows Monitor Manager Service

[中文文档](README_zh-CN.md) | **English**

A powerful Windows Service application written in Rust that provides a RESTful API and Web UI to manage physical monitors and display topology remotely.

It is designed to work robustly even in **Session 0 (Windows Service)** and at the **Login Screen (WinLogon)**, solving common issues where display APIs fail when no user is logged in.

## ✨ Features

*   **Session 0 Isolation Bypass**: Uses advanced token stealing and process snapshotting (similar to RustDesk) to inject an agent into the active console session (User Desktop or Login Screen).
*   **Works at Login Screen**: Can enumerate and switch displays even when no user is logged in.
*   **Web UI**: Embedded modern Web UI for easy management from any device (phone/laptop).
*   **Single Binary**: All components (Service, API, Agent) are packed into a single executable `monitor-service.exe`.
*   **Robust Display Control**: Uses Windows **CCD** (Connecting and Configuring Displays) API instead of legacy GDI for reliable monitor identification and topology switching.
*   **Smart Filtering**: Automatically hides phantom "Generic Non-PnP Monitors" and inactive displays.
*   **Exclusive Activation**: Supports "Show Only on X" mode by strictly disabling all other display paths.

## 🚀 Quick Start

### 1. Console Mode (Testing)
Run directly in a terminal to test functionality without installing as a service.

```powershell
.\monitor-service.exe --addr 0.0.0.0:3000
```
Then open `http://localhost:3000` in your browser.

### 2. Install as Windows Service
To have it run automatically at boot (and work at the login screen):

1.  Open PowerShell as **Administrator**.
2.  Create the service (Replace path with your actual path):
    ```powershell
    sc.exe create MonitorService binPath= "C:\path\to\monitor-service.exe --service --addr 0.0.0.0:3000" start= auto
    ```
    > **Note**: The space after `binPath=` is required.
3.  Start the service:
    ```powershell
    sc.exe start MonitorService
    ```

### 3. Stop/Uninstall
```powershell
sc.exe stop MonitorService
sc.exe delete MonitorService
```

## 🛠️ Usage

### Web Interface
Visit `http://YOUR_PC_IP:3000` to:
*   View list of active physical monitors.
*   Click a monitor card to **activate ONLY that monitor**.
*   Use quick actions: **Internal** (PC Screen), **External** (Second Screen), **Extend**, **Duplicate**.

### Command Line Arguments
```text
Usage: monitor-service.exe [OPTIONS] [COMMAND]

Options:
  --service             Run as a Windows Service
  --addr <ADDR>         IP address and port to listen on [default: 0.0.0.0:3000]
  -h, --help            Print help

Commands:
  agent                 Run as Agent (Internal use for session injection)
```

## 🏗️ Build Instructions

Requirements: `rustup`, `cargo`.

1.  Clone the repository.
2.  Build for release:
    ```bash
    cargo build --release
    ```
3.  The binary will be at `target/release/monitor-service.exe`.

## 🧩 Architecture

*   **Service Process (Session 0)**: Runs the Axum Web Server and handles API requests. It detects the active console session ID.
*   **Agent Process (User Session)**: The Service uses `CreateProcessAsUserW` to launch itself (`monitor-service.exe agent`) inside the active user session (or WinLogon session).
*   **Communication**: The Service captures the `stdout` of the Agent process to get display lists and results.

## ⚠️ Troubleshooting

*   **"No monitors found"**: Ensure the service is running and you are not in a Remote Desktop (RDP) session (RDP uses a virtual display driver). This tool manages **physical** GPU outputs.
*   **Duplicate Monitors**: The tool automatically filters duplicates based on hardware Target IDs. If you still see duplicates, they might be ghost entries from the driver; try restarting the PC.

## Acknowledgements

*   Thanks to [RustDesk](https://github.com/rustdesk/rustdesk).

