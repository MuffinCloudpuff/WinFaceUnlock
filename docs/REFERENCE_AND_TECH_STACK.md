# FaceWinUnlock-Tauri 参考边界与技术栈取舍

更新时间：2026-05-31

## 一、结论

WinFaceUnlock 可以沿着 FaceWinUnlock-Tauri 的 **Windows 解锁技术路线** 继续做：

```text
Credential Provider DLL
-> 本机 IPC / 命名管道
-> 后台认证 Agent
-> 人脸识别通过后触发 Windows 解锁
```

是否直接使用，需要按目标区分：

1. **直接安装官方发布版做本机体验或基线验证：可以。**
2. **把最新版官方发布版作为可修改底座：不行。** 主分支从 2026-03-01 后已经删除大量核心源码。
3. **基于旧开源分支二次开发：技术上可以。** 但旧分支存在明文用户名密码管道传输等安全问题，而且缺少后续版本修复。
4. **复制旧源码并作为闭源自有实现发布：不建议。** 项目许可证是 AGPL-3.0，需要先满足相应开源合规义务。

推荐策略：

1. **先在虚拟机直接安装官方发布版，验证独立解锁体验和兼容性。**
2. **把基础独立模式做成与上游一致的锁屏后鼠标或键盘输入触发。**
3. **长期如果需要接入小车视频源、雷达触发或自定义安全策略，再选择旧分支 fork 后加固，或者独立实现。**
4. **第一版核心常驻服务优先使用 Rust。**
5. **Credential Provider 层必然涉及 DLL/COM，不因选择 Rust 而消失。**

这里的“效果最优”优先级定义为：

1. Windows 登录链路正确、可恢复。
2. 安全边界清晰，不让远端信号直接解锁。
3. 常驻后台足够轻量，空闲时 CPU 接近 0。
4. 模型识别效果、速度和误识别率可调优。
5. 工程可维护，后续模型和视频源可替换。

## 二、哪些部分可以参考

### 2.1 Credential Provider 工程结构

可参考程度：高。

可参考内容：

1. `Server/src/lib.rs`
   - COM DLL 入口。
   - `DllGetClassObject`。
   - `DllCanUnloadNow`。
   - 类工厂 `IClassFactory`。
   - 注册表读取。
   - DLL 日志初始化。

2. `Server/src/CSampleProvider.rs`
   - `ICredentialProvider` 实现方式。
   - `SetUsageScenario` 保存当前登录/解锁场景。
   - `Advise` / `UnAdvise` 生命周期。
   - `GetCredentialCount` 如何触发自动登录。
   - `GetCredentialAt` 如何创建 Credential Tile。

3. `Server/src/CSampleCredential.rs`
   - `ICredentialProviderCredential` 实现方式。
   - `GetSerialization` 的核心职责。
   - `CredPackAuthenticationBufferW` 打包用户名密码。
   - `CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION` 填充方式。
   - `ReportResult` 处理登录失败反馈。

我们要怎么用：

1. 参考它的 COM 接口实现顺序。
2. 参考 windows-rs crate feature 配置。
3. 参考 Credential Provider 生命周期。
4. 重新设计命名、CLSID、日志、注册表路径和错误处理。

不要直接照搬：

1. GUID。
2. 管道名称。
3. 日志路径。
4. 业务配置 key。
5. 明文凭据传输逻辑。

### 2.2 命名管道通信

可参考程度：中。

可参考内容：

1. `Server/src/Pipe.rs`
   - `CreateNamedPipeW`。
   - `ConnectNamedPipe`。
   - `WaitNamedPipeW`。
   - `CreateFileW`。
   - `ReadFile` / `WriteFile`。

2. `Server/src/CPipeListener.rs`
   - Credential Provider 内部启动后台监听线程。
   - 收到管道消息后更新共享凭据。
   - 调用 `CredentialsChanged` 通知 Windows。

我们要怎么用：

1. 可以继续使用 Windows 命名管道作为本机 IPC。
2. 需要加安全描述符 / ACL，只允许当前用户、SYSTEM 或指定服务访问。
3. 管道协议要结构化，不用字符串拼接。
4. 管道消息不能长期携带明文 Windows 密码。

不要直接照搬：

```text
username::FaceWinUnlock::password
```

这个协议太脆弱，且没有认证、过期、nonce、防重放和 ACL 约束。

### 2.3 自动触发识别的思路

可参考程度：中。

旧分支里 `CPipeListener.rs` 使用键盘/鼠标 hook 判断用户在锁屏界面有操作，然后通知前台 Agent 开始识别。

可参考的不是 hook 代码本身，而是这个产品思路：

```text
不要一直识别
-> 先等触发条件
-> 再打开摄像头 / 加载模型 / 开始识别
```

我们的触发源优先级应是：

1. 基础模式：Windows 锁屏后的本地鼠标或键盘输入。
2. 可选增强：小车 24G 毫米波雷达有人。
3. 可选增强：小车视频源可用。

第一版需要实现本地键鼠触发，但要控制范围：只服务于锁屏解锁场景，并在虚拟机中验证 hook 生命周期、误触发和安全软件兼容性。

### 2.4 OpenCV YuNet + SFace 模型链路

可参考程度：高。

旧分支里能看到：

1. `FaceDetectorYN` 加载 `face_detection_yunet_2023mar.onnx`。
2. `FaceRecognizerSF` 加载 `face_recognition_sface_2021dec.onnx`。
3. 先检测人脸，再 `align_crop`，再提取 feature。
4. 用余弦相似度做模板匹配。
5. 连续多次成功后才算通过。

这条链路适合第一版：

1. 模型轻。
2. 部署简单。
3. CPU 可跑。
4. OpenCV 文档和样例较多。

但要拆成独立 provider，再通过组合 pipeline 接入上层：

```text
FaceDetectionModelProvider
  detect(frame) -> face boxes / landmarks

FaceRecognitionModelProvider
  extract(frame, face) -> embedding
  compare(a, b) -> score

FaceModelPipeline
  detector: FaceDetectionModelProvider
  recognizer: FaceRecognitionModelProvider
```

不要让上层策略知道 YuNet/SFace 的具体类型。检测模型和识别模型必须可以单独替换。替换识别模型后，旧 embedding 模板不得静默参与新模型比对；模板需要记录识别模型族和版本，并重新注册和校准阈值。

### 2.5 安装、卸载、注册表与计划任务

可参考程度：中。

可参考内容：

1. Tauri 项目里的初始化/卸载流程。
2. 注册表写入思路。
3. 计划任务启动方式。
4. 卸载时清理 Credential Provider 和缓存。

需要重做的部分：

1. 安装脚本必须先以虚拟机为目标验证。
2. 卸载脚本必须能恢复系统登录。
3. 注册表路径、服务名、任务名必须换成 WinFaceUnlock 自己的命名。
4. 所有操作要有 dry-run 或确认日志。

## 三、哪些部分不建议参考或复用

### 3.1 直接复用它的 UI

不建议。

我们的 UI 入口应该在 `PC_Client` 控制中心里做联动展示，而不是单独复制一个 Vue/Tauri 设置界面。

### 3.2 直接复用它的数据库 schema

不建议。

它的 `faces` 表里保存了 `user_name`、`user_pwd`、`face_token` 等字段。我们不能把 Windows 密码作为普通业务字段保存。

我们的配置应该拆成：

1. 人脸模板。
2. 用户映射。
3. 解锁策略。
4. 受保护凭据引用。
5. 日志。

### 3.3 直接复用明文管道协议

不建议。

旧实现是为了跑通功能，不适合作为长期安全边界。

### 3.4 直接使用小车端认证凭证

不建议。

小车端只做视频源和 presence trigger，不作为认证权威。

## 四、语言与技术栈取舍

### 4.1 常驻后台服务的要求

WinFaceUnlock 的常驻部分必须满足：

1. 内存占用低。
2. 空闲时 CPU 接近 0。
3. 启动快。
4. 可打包为 Windows 后台进程或服务。
5. 与 Win32 API / Credential Provider / 命名管道集成可靠。
6. 崩溃后可恢复，不影响系统登录。

### 4.2 Rust

推荐程度：高。

适合模块：

1. Credential Provider DLL。
2. UnlockBroker。
3. 本机 IPC。
4. Agent supervisor。
5. 配置、状态机和轻量策略。

优点：

1. 无 GC，常驻内存可控。
2. 性能和 C++ 接近。
3. 比 C/C++ 更容易避免内存安全问题。
4. `windows-rs` 对 Win32 / COM 支持较完整。
5. 和 FaceWinUnlock-Tauri 的 Windows 技术路线一致。

缺点：

1. Credential Provider / COM 调试门槛高。
2. OpenCV Rust 绑定编译环境较重。
3. 生态里现成的人脸识别封装不如 Python 顺手。

建议：

```text
第一版 Windows 解锁核心用 Rust。
人脸识别也用 Rust/OpenCV 验证和实现。
不使用 Python sidecar。
```

### 4.2.1 Rust 是否涉及 DLL

涉及。

Windows Credential Provider 本质是一个 COM in-process server，系统登录界面会加载注册到系统里的 DLL。无论使用 C++、C、Rust，最终都要产出一个 Windows DLL，并实现 Credential Provider 相关 COM 接口。

Rust 对应方式是：

```text
Rust crate-type = ["cdylib"]
-> 导出 DllGetClassObject
-> 导出 DllCanUnloadNow
-> 实现 IClassFactory
-> 实现 ICredentialProvider
-> 实现 ICredentialProviderCredential
```

也就是说：

1. Rust 可以写 Credential Provider。
2. 产物仍然是 DLL。
3. 关键不是语言名字，而是能否稳定实现 COM 接口和 Winlogon 生命周期。
4. FaceWinUnlock-Tauri 正是用 Rust 走这条路线。

项目拆分建议：

```text
windows_provider/
  Rust cdylib，注册给 Windows 登录系统加载。

unlock_broker/
  Rust exe，负责本机 IPC、短时授权、凭据保护和状态管理。

face_agent/
  Rust exe，负责视频帧和模型识别。
```

不要把所有东西都塞进 DLL。Credential Provider DLL 应尽量小，只做和 Winlogon 交互必须做的事情。模型推理、视频读取、雷达状态、复杂策略都应放在外部 Agent / Broker 中。

### 4.3 C++

推荐程度：中高。

适合模块：

1. Credential Provider DLL。
2. OpenCV 推理。
3. 极致轻量的本地 Agent。

优点：

1. Windows Credential Provider 传统资料最多。
2. OpenCV C++ 原生支持最好。
3. 性能强，体积可控。

缺点：

1. 内存安全和异常边界要自己负责。
2. 工程维护成本比 Rust 高。
3. 和现有讨论里参考项目的 Rust 路线不完全一致。

建议：

如果 Rust OpenCV 绑定遇到持续阻塞，可以考虑：

```text
Rust: Credential Provider + Broker
C++: Face engine 动态库
```

### 4.4 Go

推荐程度：中。

适合模块：

1. 轻量后台 Agent。
2. 本机 HTTP/WebSocket/配置服务。
3. 与 PC_Client 的状态联动。

优点：

1. 开发快。
2. 单文件分发方便。
3. 常驻服务稳定。

缺点：

1. Win32 COM / Credential Provider 不适合 Go。
2. OpenCV / ONNX 生态不如 C++ / Python / Rust。
3. 带 GC，虽然通常不重，但不是极致轻量。

建议：

Go 可以做外围 agent，但不建议做 Credential Provider。

### 4.5 .NET / C#

推荐程度：Agent 中等，Credential Provider 低。

适合模块：

1. PC 端配置工具。
2. 后台状态面板。
3. 和 `PC_Client` 联动的小型本机服务。
4. 管理安装、卸载、日志查看等外围能力。

优点：

1. Windows 平台集成体验好。
2. 开发效率高。
3. WPF / WinUI / MAUI 做配置界面方便。
4. 调用 Windows API、DPAPI、Credential Manager、Event Log 等系统能力较顺手。
5. 如果使用 NativeAOT，常驻体积和启动速度可以比传统 .NET 好。

缺点：

1. Credential Provider 是原生 COM DLL 场景，.NET 直接做 in-process Provider 不推荐。
2. Winlogon 加载托管运行时会带来复杂性、启动成本和可靠性风险。
3. .NET 常驻服务通常比 Rust/C++ 更重，虽然对普通后台服务可能可以接受。
4. OpenCV/ONNX 可以接，但最终轻量程度不如 Rust/C++ 可控。

判断：

```text
.NET 可以做管理端、配置端、日志端。
.NET 不建议做 Credential Provider DLL。
.NET 可以做 Agent PoC，但不是本项目“极致轻量”主线。
```

如果后续需要一个 Windows 原生配置程序，.NET 是合理选择。但当前我们已经有 `PC_Client` 控制中心，第一版不必再做单独 .NET UI。

### 4.6 Python

推荐程度：不纳入本项目路线。

当前决策：

1. 不使用 Python 做模型 PoC。
2. 不使用 Python 做 FaceAuth sidecar。
3. 不使用 Python 做常驻后台。
4. 不把现有 `PC_Client` Python 主服务混入解锁主链路。

原因：

1. 本项目要求常驻轻量、可打包、可开机未登录运行。
2. Credential Provider、Service、DPAPI、named pipe、ACL 都更适合 Rust/MSVC 主线。
3. 继续保留 Python 选项会导致实现路线摇摆。

## 五、推荐最终技术路线

### 5.1 主推荐：Rust-first

```text
Rust Agent
  - supervisor
  - config
  - policy state machine
  - presence provider
  - video frame provider
  - face model provider
  - unlock broker

Rust Credential Provider DLL
  - Winlogon integration
  - secure named pipe client/server
  - serialization
```

适用条件：

1. Rust OpenCV / ONNX Runtime 接入顺利。
2. 模型性能满足要求。
3. 打包体积可接受。

这是长期最干净的路线。

### 5.2 已放弃过渡路线：双运行时 FaceAuth Sidecar

曾考虑用独立脚本运行时先做人脸识别验证，但该路线已经放弃。

放弃原因：

1. 增加双运行时、双进程和打包复杂度。
2. 不利于开机未登录场景。
3. 与“Rust 主线、可维护、轻量常驻”的目标冲突。

### 5.3 备选：Rust + C++ Face Engine

```text
Rust
  - Credential Provider
  - UnlockBroker
  - policy
  - IPC

C++
  - OpenCV face detection
  - face recognition
  - liveness
```

适用条件：

1. Rust OpenCV 绑定不稳定。
2. 需要 OpenCV C++ 原生性能和稳定性。
3. 接受维护一层 C ABI / DLL 边界。

### 5.4 .NET 的定位

不作为主线。

推荐定位：

```text
可选：
- 安装器辅助工具
- 管理面板
- 日志查看工具
- DPAPI / Credential Manager 辅助验证 PoC

不推荐：
- Credential Provider DLL
- 核心常驻识别服务
- Winlogon 进程内逻辑
```

原因不是 .NET 不能做 Windows 程序，而是本项目最敏感的部分在 Winlogon 和常驻轻量后台。这里 Rust/C++ 的控制力更强。

## 六、第一阶段建议

第一阶段不要直接进入 Credential Provider。

建议顺序：

1. 用 Rust 快速验证本地摄像头视频帧是否能稳定做人脸检测。
2. 固化 `FaceModelProvider` contract。
3. 固化鼠标/键盘触发和锁屏状态 contract。
4. 用 Rust 写轻量 supervisor 和状态机。
5. 再做 Rust UnlockBroker。
6. 在虚拟机做 Credential Provider。
7. 最后扩展小车视频源和雷达触发。

这样可以避免一开始被 Winlogon 调试拖慢。

## 七、最终判断

是否继续沿着 FaceWinUnlock-Tauri 的 Rust 技术路线？

答案是：**Windows 解锁层应该继续沿着 Rust 路线走。**

人脸识别层也固定为 Rust/OpenCV 主线。

长期目标：

```text
常驻轻量 supervisor：Rust
Windows 解锁核心：Rust
模型推理：优先 Rust，必要时 C++ DLL
Python：不进入本项目实现路线
.NET：只作为可选管理工具，不进入 Credential Provider 主链路
```

最终产物形态应是：

```text
Credential Provider DLL: Rust cdylib
UnlockBroker: Rust exe / Windows service-like background process
FaceUnlockAgent: 优先 Rust，必要时 Rust + C++ face engine
PC_Client integration: 现有 PC_Client 控制中心
可选管理工具: .NET / Rust / Tauri，后续再定
```
