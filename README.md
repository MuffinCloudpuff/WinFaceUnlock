<div align="center">

[English](./README_EN.md) | [中文](./README.md)

# 🪪 WinFaceUnlock

**Windows 本地人脸解锁与离座锁屏工具**

[![OS](https://img.shields.io/badge/OS-Windows_10_|_11-blue?logo=windows)](#)
[![Rust](https://img.shields.io/badge/Built_with-Rust-orange?logo=rust)](#)
[![License](https://img.shields.io/badge/License-Apache_2.0-green.svg)](#)

</div>

**WinFaceUnlock** 是一个基于 Rust 开发的 Windows 原生人脸解锁解决方案。它的目标很直接：**让 Windows 登录更方便，同时把安全边界严格收紧在本机**。所有密码、人脸特征和推理过程均在本地高权限服务中闭环，不依赖云端，不将密码作为普通数据暴露。

---

## ✨ 核心特性

- ⚡ **无感人脸解锁**：在 Windows 锁屏/登录界面自动调用摄像头进行精准的人脸识别与解锁。
- 🚶 **离座自动锁屏**：后台持续检测，当检测到用户离开屏幕前一定时间后，自动锁定电脑保护隐私。
- 🛡️ **本地安全隔离**：基于 Windows 原生 Credential Provider 链路完成登录交接。密码加密落盘，明文只在受控的本地内存/短时 IPC 通道中短暂存在。
- 🧑‍💻 **基础防伪活体检测**：内置轻量级非活体防护，提升安全性（注：非活体检测是增强能力，非绝对保证）。
- 🎨 **现代化控制面板**：基于 Tauri 构建的轻量级前端，提供流畅的人脸录入和策略配置体验。

## 🏗️ 架构概览

本项目由三个核心部分组成，确保权限隔离与系统稳定：

1. **WinFaceUnlockService (Rust)**：作为 `SYSTEM` 权限的 Windows 后台服务运行。负责拉起摄像头、运行 ONNX 模型（YOLOv8 + GhostFaceNet）进行人脸识别，并安全存储比对凭据。
2. **Credential Provider (C++)**：以动态链接库形式嵌入 Windows `LogonUI`。在锁屏界面与后台服务进行 IPC 通信，安全完成登录凭据的握手。
3. **Control App (Tauri/React)**：以普通用户权限运行的图形界面。通过安全的命名管道与服务通信，完成人脸录入、阈值调节和开关设置。

## 🚀 安装与使用

1. **下载安装包**：获取最新编译的 `WinFaceUnlockSetup.exe`。
2. **执行安装**：双击运行，安装程序将自动注册 Windows 凭据提供程序并启动后台服务。
3. **录入人脸**：在桌面右下角托盘找到 WinFaceUnlock 图标，打开主面板，点击“录入人脸”。
4. **体验解锁**：按下 `Win + L` 锁屏，面对摄像头即可体验秒级解锁！

---

## ⚠️ 免责声明

本项目涉及 Windows 登录、锁屏、Credential Provider 和本地系统服务等极其敏感的系统行为。在使用或二次开发前，请务必了解：

1. **高风险操作**：错误的安装、配置或二次开发可能导致系统**无法正常登录**。
2. **测试建议**：强烈建议先在 VMware、Hyper-V 等虚拟机环境中调试和验证。
3. **保留后路**：请务必保留可用的 Windows PIN、密码、管理员账号或其他系统恢复手段。
4. **环境警告**：请勿在生产环境、重要工作电脑或保存关键数据的机器上直接实验未经验证的版本。
5. **免责条款**：作者不对因使用、修改、分发或二次开发本软件导致的任何数据丢失、系统崩溃、无法登录、安全漏洞或其他损失承担责任。

## 📜 开源协议与致谢

本仓库根目录当前采用 [Apache License 2.0](LICENSE) 开源。

**致谢与参考项目**：  
本项目在早期探索阶段参考了 [FaceWinUnlock-Tauri](https://github.com/zs1083339604/FaceWinUnlock-Tauri)。我们在此基础上，围绕更明确的本地凭据边界、IPC 授权语义、离座锁屏及系统服务化进行了重新设计与彻底的 Rust 重构。
*(注：仓库中保留的参考项目 `FaceWinUnlock-Tauri-main/` 使用其自身的 AGPL-3.0 许可证。)*
