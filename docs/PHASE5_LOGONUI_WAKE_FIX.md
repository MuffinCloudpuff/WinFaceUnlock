# Phase 5 LogonUI 唤醒卡顿问题复盘

## 背景

Phase 5 的目标是在虚拟机中验证 Windows Credential Provider 链路。开启“无需点击磁贴即可自动做人脸登录”的实验后，暴露出两个问题：

1. `AutoWakeOnAdvise` 如果默认启用，LogonUI 枚举 Provider 时就会立刻启动摄像头认证。
2. wake/auth 请求在 Credential Provider 回调里同步执行，所以摄像头被挡住、不可用，或者读取有效画面较慢时，LogonUI 会变慢，甚至短时间卡住。

这两个问题属于 Provider 集成方式问题，不是人脸识别模型精度问题。无论 WinFaceUnlock 处于什么状态，PIN、密码和 Windows Hello 兜底登录方式都必须保留。

## 现象

- 开启无点击自动唤醒后，进入锁屏登录界面的速度变慢。
- 摄像头被遮挡时，用户想切换到 PIN 或 Windows 原生登录界面，LogonUI 仍然会延迟响应。
- 磁贴和兜底路径都还在，但 Provider 回调会阻塞用户操作，让兜底路径看起来不够可靠。

## 根因

第一版实现直接在以下 Credential Provider 回调中调用 Service wake 链路：

- `ICredentialProvider::Advise`
- `ICredentialProviderCredential::SetSelected`

这条 wake 链路会通过 IPC 调用 `WinFaceUnlockService`。Service 可能会打开摄像头、读取帧、人脸检测、人脸识别，然后再取受保护的凭据材料。

这些操作不适合放在 LogonUI 回调线程里执行。即使摄像头和模型推理已经放在 Service 进程中，Provider 仍然在同步等待 Service 返回结果。只要摄像头链路慢，或者失败路径返回得晚，LogonUI 就会一起等待。

## 修复方案

### 1. 自动唤醒改成显式开关

`AutoWakeOnAdvise` 现在默认是 `false`。

默认行为：

- WinFaceUnlock 磁贴仍然可见。
- 用户选择磁贴时可以启动人脸登录。
- Windows PIN、密码、Windows Hello 不会被修改。
- LogonUI 只是枚举 Provider 时，不会自动启动摄像头或认证链路。

如果要在虚拟机中实验“无点击自动登录”，仍然可以显式开启：

```powershell
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll --auto-wake-on-advise
```

### 2. 把 wake/auth 从 LogonUI 回调线程移到后台线程

`Advise` 和 `SetSelected` 现在调用 `request_wake_in_background`，不再同步执行 wake/auth 链路。

Provider 回调现在只做轻量工作：

1. 尝试把 Provider 状态切换到 `WakeRequested`。
2. 启动一个命名后台 worker 线程。
3. 立即把控制权还给 LogonUI。

后台 worker 线程负责：

1. 读取 Provider 运行配置。
2. 通过本机 IPC 调用 `WinFaceUnlockService`。
3. 等待人脸认证和受保护凭据材料。
4. 更新 Provider 状态。
5. 通过 `CredentialsChanged` 通知 LogonUI 刷新凭据。

这样人脸识别可以继续在后台跑，同时 Windows 原生 PIN/密码兜底操作不会被摄像头链路阻塞。

### 3. 用 Agile COM 引用通知 LogonUI

`ICredentialProviderEvents` 是 COM 接口，不能直接把原始对象跨 Rust 线程移动。Provider 现在把事件回调保存为：

```rust
AgileReference<ICredentialProviderEvents>
```

后台 worker 只有在需要调用 `CredentialsChanged` 时才解析这个 agile 引用。这样既能在后台线程完成后刷新 LogonUI，又避免了不安全的跨线程 COM 指针共享。

### 4. 增加 Provider 状态保护

Provider 状态机还增加了几类保护：

- wake 请求正在运行时，不允许重复启动第二个 wake 请求。
- wake 失败后进入短暂冷却，避免立刻重复触发。
- 凭据材料成功序列化后立刻消费掉，避免同一份内存密码重复触发自动登录。
- 敏感凭据材料的 Debug 输出脱敏。
- 传给 `CredPackAuthenticationBufferW` 的 UTF-16 密码缓冲区在使用后清零。

## 为什么这样解决

把摄像头和人脸识别塞进 Provider 会让系统更脆弱，因为 LogonUI 会加载 Provider DLL，而 LogonUI 是敏感登录进程。正确边界应该是：

- Provider：只做 Windows 集成、状态、磁贴、凭据序列化和通知。
- Service：负责摄像头、模型推理、认证策略、Credential Store 和 IPC。

这次问题不是“识别逻辑放在 Service”导致的，而是 Provider 同步等待 Service。后台 wake worker 保留了既有模块边界，同时消除了用户可感知的 LogonUI 阻塞。

使用 `AgileReference` 比手动跨线程共享 COM 指针更合适，因为它把跨线程通知契约显式化，并由 `windows-core` 提供类型层面的约束。

## 验证

本地验证：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p windows_provider -p installer_cli -p win_service -p diagnostics_cli
```

修复版部署到虚拟机后的状态：

```text
WinFaceUnlockProvider registered: true
WinFaceUnlockService status: Running
WinFaceUnlockService health-check: HealthOk
TileVisibility: visible
AutoWakeOnAdvise: true
WakeAuthSource: local-camera
```

虚拟机中预期行为：

- `AutoWakeOnAdvise=true` 时，LogonUI 可以自动启动人脸认证。
- 人脸认证运行时，LogonUI 仍应保持响应。
- 遮挡摄像头时，不应阻塞切换到 PIN/密码兜底登录。
- 人脸认证失败后，不应进入立即重试循环。

## 恢复规则

如果 Provider 在虚拟机中出现不稳定，优先只禁用 WinFaceUnlock Provider 枚举：

```powershell
.\installer_cli.exe emergency-disable-provider
```

这个恢复命令不能修改 Windows PIN、密码、Windows Hello、账户策略或其它 Credential Provider。
