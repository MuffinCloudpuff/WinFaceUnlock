# WinFaceUnlock

WinFaceUnlock 是一个面向 Windows 的本地人脸解锁产品，专注于人脸录入、锁屏解锁、离座自动锁屏、非活体防护和本地凭据保护。

WinFaceUnlock is a local face unlock product for Windows, focused on face enrollment, lock-screen sign-in, automatic locking when the user leaves, basic anti-spoofing, and local credential protection.

它的目标很直接：让 Windows 登录更方便，同时尽量把安全边界收紧在本机，不依赖云端，不把密码当普通数据到处传。

Its goal is simple: make Windows sign-in more convenient while keeping the security boundary on the local machine, without cloud dependency or treating passwords like ordinary transferable data.

## 我们实现了什么 / What We Built

- 人脸录入与本地模板管理
- Windows 锁屏/登录场景的人脸解锁
- 本地摄像头识别
- 离座自动锁屏
- 基础非活体防护
- 本地控制面板与设置管理
- 安装、诊断、调试工具链

- Face enrollment and local template management
- Face unlock for Windows lock-screen and sign-in scenarios
- Local camera recognition
- Automatic locking when the user leaves
- Basic anti-spoofing protection
- Local control panel and settings management
- Installation, diagnostics, and debugging tools

## 安全特性 / Security Highlights

- 基于 Windows 原生凭据链路完成最终登录交接
- 密码不明文落盘
- 密码不走普通明文接口
- 明文只在受控的本地内存/短时通道中短暂存在
- 非活体检测是增强能力，不是绝对保证

- Final sign-in handoff is based on the native Windows credential flow
- Passwords are not stored on disk in plaintext
- Passwords are not sent through ordinary plaintext interfaces
- Plaintext only exists briefly in controlled local memory or short-lived local channels
- Anti-spoofing is a risk-reduction feature, not an absolute security guarantee

## 适合谁用 / Who It Is For

- 想在 Windows 上体验本地人脸登录/解锁的人
- 想要离座后自动锁屏的人
- 想让人脸模板和登录凭据尽量留在本机的人
- 想在虚拟机里验证 Windows 登录链路的人

- People who want local face sign-in/unlock on Windows
- People who want their PC to lock automatically when they leave
- People who want face templates and login credentials to stay on the local machine
- Developers who want to validate Windows sign-in flows in a virtual machine

## 免责声明 / Disclaimer

本项目涉及 Windows 登录、锁屏、Credential Provider 和本地系统服务等敏感系统行为。在使用或二次开发前，请务必了解：

This project touches sensitive Windows behaviors such as sign-in, lock screen integration, Credential Provider, and local system services. Before using or modifying it, please understand:

1. 错误的安装、配置、卸载或二次开发操作可能导致系统无法正常登录。
2. 强烈建议先在 VMware、Hyper-V 等虚拟机环境中调试和验证。
3. 请保留可用的 Windows PIN、密码、管理员账号或系统恢复手段。
4. 请勿在生产环境、重要工作电脑或保存关键数据的机器上直接实验未经验证的版本。
5. 作者不对因使用、修改、分发或二次开发本软件导致的任何数据丢失、系统崩溃、无法登录、安全漏洞或其他损失承担责任。

1. Incorrect installation, configuration, uninstallation, or modification may prevent the system from signing in normally.
2. Debugging and validation in VMware, Hyper-V, or another virtual machine environment is strongly recommended.
3. Keep a working Windows PIN, password, administrator account, or system recovery method available.
4. Do not test unverified builds directly on production machines, important work computers, or devices storing critical data.
5. The author is not responsible for any data loss, system crash, sign-in failure, security issue, or other damage caused by using, modifying, distributing, or developing this software.

## 开源协议 / License

本仓库根目录当前采用 [Apache License 2.0](LICENSE) 开源。

The repository root is currently licensed under the [Apache License 2.0](LICENSE).

仓库中保留的参考项目 `FaceWinUnlock-Tauri-main/` 使用其自身的 AGPL-3.0 许可证；如果复制、修改或分发该参考项目代码，需要遵守该目录内的许可证要求。

The retained reference project under `FaceWinUnlock-Tauri-main/` uses its own AGPL-3.0 license. If you copy, modify, or distribute code from that directory, you must follow the license terms inside that directory.

## 参考项目 / Reference Project

- [FaceWinUnlock-Tauri](https://github.com/zs1083339604/FaceWinUnlock-Tauri)

该项目对 Windows Credential Provider、命名管道、OpenCV 人脸识别链路有参考价值。本项目围绕更明确的本地凭据边界、认证授权语义、离座锁屏和虚拟机验证流程进行了重新设计与实现。

This project is a useful reference for Windows Credential Provider, named pipes, and OpenCV-based face recognition flows. WinFaceUnlock redesigns and reimplements the product around clearer local credential boundaries, explicit authentication semantics, automatic presence locking, and virtual machine validation.
