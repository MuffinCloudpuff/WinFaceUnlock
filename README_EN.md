<div align="center">

[English](./README_EN.md) | [中文](./README.md)

# 🪪 WinFaceUnlock

**A Local Face Unlock & Presence Monitoring Tool for Windows**

[![OS](https://img.shields.io/badge/OS-Windows_10_|_11-blue?logo=windows)](#)
[![Rust](https://img.shields.io/badge/Built_with-Rust-orange?logo=rust)](#)
[![License](https://img.shields.io/badge/License-Apache_2.0-green.svg)](#)

</div>

**WinFaceUnlock** is a local face unlock product for Windows built with Rust. Its goal is simple: **make Windows sign-in more convenient while keeping the security boundary strictly on the local machine**. All credentials, face features, and inference run in a secure local service without cloud dependencies, and passwords are never exposed as plain data.

---

## ✨ Features

- ⚡ **Seamless Face Unlock**: Automatically triggers the camera for fast and accurate face recognition at the Windows lock-screen and sign-in screen.
- 🚶 **Auto-Lock on Away**: Continuously monitors presence in the background and automatically locks the PC when the user leaves to protect privacy.
- 🛡️ **Local Security First**: Final sign-in handoff is based on the native Windows Credential Provider flow. Passwords are encrypted on disk and only exist briefly in controlled local memory or short-lived local IPC channels.
- 🧑‍💻 **Basic Anti-Spoofing**: Built-in lightweight anti-spoofing to reduce the risk of photo attacks (Note: this is a risk-reduction feature, not an absolute security guarantee).
- 🎨 **Modern Control Panel**: A lightweight frontend built with Tauri and React, providing a smooth experience for enrolling faces and adjusting threshold settings.

## 🏗️ Architecture

The project consists of three core components to ensure privilege isolation and system stability:

1. **WinFaceUnlockService (Rust)**: Runs as a `SYSTEM` privilege background service. It is responsible for accessing the camera, running ONNX models (YOLOv8 + GhostFaceNet) for face recognition, and securely storing comparison credentials.
2. **Credential Provider (C++)**: Embedded as a dynamic link library into the Windows `LogonUI`. It communicates with the background service via IPC on the lock screen to securely complete the login credential handshake.
3. **Control App (Tauri/React)**: A graphical interface running with standard user privileges. It communicates with the service via secure named pipes to handle face enrollment, threshold adjustments, and toggle settings.

## 🚀 Installation & Usage

1. **Download the Installer**: Get the latest compiled `WinFaceUnlockSetup.exe`.
2. **Install**: Double-click to run. The installer will automatically register the Windows Credential Provider and start the background service.
3. **Enroll Face**: Find the WinFaceUnlock icon in the system tray, open the main panel, and click "Enroll Face" (录入人脸).
4. **Experience Unlock**: Press `Win + L` to lock your screen, face the camera, and enjoy the seamless unlock experience!

---

## ⚠️ Disclaimer

This project touches extremely sensitive Windows behaviors, such as sign-in, lock screen integration, Credential Provider, and local system services. Before using or modifying it, please understand:

1. **High-Risk Operations**: Incorrect installation, configuration, or modification may prevent the system from signing in normally.
2. **Testing Recommendation**: Debugging and validation in VMware, Hyper-V, or another virtual machine environment is strongly recommended first.
3. **Keep Backups**: Please ensure you have a working Windows PIN, password, administrator account, or system recovery method available.
4. **Environment Warning**: Do not test unverified builds directly on production machines, important work computers, or devices storing critical data.
5. **Limitation of Liability**: The author is not responsible for any data loss, system crash, sign-in failure, security issue, or other damage caused by using, modifying, distributing, or developing this software.

## 📜 License & Acknowledgements

The repository root is currently licensed under the [Apache License 2.0](LICENSE).

**Acknowledgements**:  
During its early exploration phase, this project referenced [FaceWinUnlock-Tauri](https://github.com/zs1083339604/FaceWinUnlock-Tauri). Based on that, WinFaceUnlock redesigned and completely rewrote the product in Rust, focusing on clearer local credential boundaries, IPC authorization semantics, auto-away locking, and system service integration.
*(Note: The retained reference project under `FaceWinUnlock-Tauri-main/` uses its own AGPL-3.0 license.)*
