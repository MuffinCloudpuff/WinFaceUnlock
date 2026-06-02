# WinFaceUnlock 详细实施方案

更新时间：2026-05-31

## 一、实施原则

本方案是 WinFaceUnlock 的工程落地顺序。所有核心代码固定使用 Rust。

明确禁止：

1. 不使用 Python PoC。
2. 不使用 Python sidecar。
3. 不把解锁核心塞进 `PC_Client`。
4. 不让小车端参与最终认证。
5. 不先写 Credential Provider 再补后台能力。

明确采用：

1. Rust workspace。
2. Windows MSVC 工具链。
3. `windows-rs` 调 Win32 / COM / DPAPI / named pipe / service API。
4. SQLCipher 加密 SQLite。
5. OpenCV YuNet + SFace。
6. LocalSystem Windows Service。
7. Rust `cdylib` Credential Provider。

## 二、开发环境准备

### 2.1 当前本机状态

当前检查结果：

```text
已可用:
  - Git

未发现或未进入 PATH:
  - rustup
  - rustc
  - cargo
  - cl.exe
  - cmake
  - ninja
  - vcpkg
```

因此编码前必须先完成环境安装。

### 2.2 必装环境

安装项：

```text
Rust:
  rustup
  stable-x86_64-pc-windows-msvc

Visual Studio Build Tools 2022:
  Desktop development with C++
  MSVC x64/x86 build tools
  Windows 10/11 SDK

Native build:
  CMake
  Ninja
  vcpkg

Tools:
  Git
  PowerShell 7 可选
  Hyper-V / VMware / VirtualBox 任一虚拟机环境
```

验证命令：

```powershell
rustc --version
cargo --version
rustup show active-toolchain
where cl
where cmake
where ninja
where vcpkg
```

验收标准：

```text
rustc 可用
cargo 可用
MSVC cl.exe 可用
CMake 可用
Ninja 可用
vcpkg 可用
能编译 x86_64-pc-windows-msvc hello world
```

### 2.3 仓库固定文件

创建项目后必须加入：

```text
rust-toolchain.toml
Cargo.lock
.cargo/config.toml
vcpkg.json 或 vcpkg-configuration.json
docs/TECHNICAL_ROUTE.md
docs/DETAILED_IMPLEMENTATION_PLAN.md
```

## 三、目标目录结构

建议结构：

```text
WinFaceUnlock/
  Cargo.toml
  rust-toolchain.toml
  .cargo/
    config.toml

  crates/
    common_protocol/
    credential_store/
    win_service/
    windows_provider/
    face_engine/
    video_provider/
    ipc/
    hardware_binding/
    installer_cli/
    diagnostics_cli/

  apps/
    config_ui/

  resources/
    models/
      face_detection_yunet_2023mar.onnx
      face_recognition_sface_2021dec.onnx
      face_liveness.onnx

  docs/
  tests/
  reference/
    FaceWinUnlock-Tauri-main/
```

说明：

1. `windows_provider` 只做 Winlogon 集成。
2. `win_service` 是主进程。
3. `credential_store` 管密钥、SQLCipher、凭据 blob。
4. `face_engine` 管模型。
5. `video_provider` 管本地摄像头和后续小车视频源。
6. `ipc` 管 named pipe、ACL 和消息协议。
7. `installer_cli` 管安装、注册、卸载、恢复。

## 四、Phase 0：工程骨架和环境锁定

目标：创建可构建的 Rust workspace。

任务：

1. 初始化 Git 仓库边界。
2. 创建 Rust workspace。
3. 添加 `rust-toolchain.toml`，固定 `stable-x86_64-pc-windows-msvc`。
4. 添加基础 crate：
   - `common_protocol`
   - `credential_store`
   - `win_service`
   - `windows_provider`
   - `face_engine`
   - `video_provider`
   - `ipc`
   - `installer_cli`
5. 添加统一错误类型和日志接口。
6. 添加 `cargo fmt`、`cargo clippy`、`cargo test` 检查。

验收：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

通过条件：

1. workspace 能完整编译。
2. 没有 Python 脚本参与构建。
3. 所有 crate 边界清楚。

## 五、Phase 1：Credential Store

目标：先把密码保护、数据库和硬件绑定跑通，不接摄像头，不接 Winlogon。

任务：

1. `credential_store` 生成 32 字节 `master_key`。
2. 使用 Windows CSPRNG 生成随机数。
3. 使用 DPAPI LocalMachine 保护 `master_key`。
4. 保存 `protected_master_key` 到受 ACL 限制的文件。
5. 接入 SQLCipher 数据库。
6. 建表：
   - `users`
   - `credentials`
   - `face_templates`
   - `policies`
   - `audit_log`
7. Windows 密码单独加密为 credential blob。
8. SQLite 只保存 `credential_ref`。
9. 实现硬件指纹采集。
10. 解密前校验硬件指纹。

关键数据结构：

```text
users:
  user_id
  user_sid
  username
  account_type
  credential_ref
  face_template_ref
  policy_id

credentials:
  credential_ref
  protected_blob_path
  created_at
  updated_at
  key_version

policies:
  policy_id
  require_liveness
  match_threshold
  failure_limit
  cooldown_seconds
```

验收：

1. 数据库文件不能用普通 SQLite 打开。
2. 拷走 `database.db` 无法直接读取内容。
3. 明文密码不出现在 SQLite 字段。
4. 明文密码不出现在日志。
5. 重启服务后仍能解开数据库。
6. 模拟硬件指纹变化时拒绝解密或进入恢复流程。

## 六、Phase 2：IPC 与短时授权

目标：建立 Service 与 Provider 将来使用的本机通信协议。

任务：

1. `common_protocol` 定义消息：
   - `WakeAuth`
   - `AuthStarted`
   - `AuthSucceeded`
   - `AuthFailed`
   - `CredentialReady`
   - `Cancel`
   - `HealthCheck`
2. `ipc` 实现 named pipe server/client。
3. 为管道设置 ACL。
4. 实现 grant：
   - `grant_id`
   - `nonce`
   - `session_id`
   - `issued_at`
   - `expires_at`
   - `used`
5. grant 只能使用一次。
6. 过期 grant 自动失效。
7. 失败次数进入 cooldown。

验收：

1. 未授权进程不能连接管道。
2. 旧 grant 不能重放。
3. grant 用一次后失效。
4. 超时后不能取凭据。
5. 日志不记录密码和完整 token。

## 七、Phase 3：Windows Service

目标：实现后台主进程，但暂时不接 Credential Provider。

任务：

1. 实现 `WinFaceUnlockService` 普通控制台模式。
2. 实现 `WinFaceUnlockService` Windows Service 模式。
3. 支持 LocalSystem 安装。
4. 启动时加载 Credential Store。
5. 启动 named pipe server。
6. 响应 `HealthCheck`。
7. 响应模拟 `WakeAuth`。
8. 模拟认证成功后返回 `CredentialReady`。
9. 实现服务停止时清理内存和管道。

验收：

1. 普通控制台模式可调试。
2. Windows Service 模式可启动、停止、重启。
3. 开机后自动启动。
4. 服务崩溃后不会破坏数据库。
5. 服务日志可读且脱敏。

## 八、Phase 4：本地摄像头和人脸识别

目标：所有识别能力用 Rust 实现。

Phase 4 必须先于 Credential Provider 自动登录实现完成。摄像头、模型推理、人脸模板、连续成功策略、失败冷却和凭据授权都应先在 Service / diagnostics CLI 环境中跑通，不能把真实摄像头或 OpenCV 推理逻辑放进 Provider DLL。

任务：

1. `video_provider` 实现本地摄像头枚举。
2. 实现本地摄像头打开、读帧、关闭。
3. `face_engine` 通过独立检测 provider 加载 YuNet。
4. `face_engine` 通过独立识别 provider 加载 SFace。
5. 可选加载 liveness 模型。
6. 实现图片注册。
7. 实现摄像头注册。
8. 实现模板保存。
9. 实现特征比对。
10. 实现连续成功策略。
11. 实现失败冷却。
12. 将真实摄像头识别链路接入 `WinFaceUnlockService` 的 `WakeAuth` 处理。
13. `diagnostics_cli wake-auth` 支持触发真实摄像头识别，而不是只走模拟认证。
14. 检测模型和识别模型通过组合 pipeline 独立热插拔；识别模板记录模型族和版本，禁止跨模型静默比对。

验收：

1. 能枚举摄像头。
2. 能取到非空帧。
3. 能检测人脸。
4. 能提取 embedding。
5. 能注册模板。
6. 能在本地摄像头下完成识别。
7. 空闲时不持续占用摄像头。
8. Service 收到 `WakeAuth` 后可拉起真实摄像头识别。
9. 真实识别成功后返回结构化 `AuthSucceeded` 和后续 `CredentialReady`。
10. 真实识别失败时返回明确失败原因，不影响下一次识别和手动登录 fallback。
11. Phase 4 未通过前，不进入 Credential Provider 自动登录实现。

## 九、Phase 5：Credential Provider 自动登录虚拟机 PoC

目标：在虚拟机中接入 Windows 登录界面，并实现类似 FaceWinUnlock-Tauri 的“登录界面自动识别，认证成功后自动提交凭据”体验。

前置条件：Phase 4 已经证明 Service 能在普通桌面环境中独立完成真实摄像头识别和授权返回。Phase 5 不再解决人脸算法正确性，只验证 Winlogon / LogonUI 生命周期、Provider 自动唤醒、`CredentialsChanged`、自动登录提交和 fallback。

路线说明：

1. 仍然使用自定义 Windows Credential Provider，不伪装成 Windows Hello 设备。
2. 磁贴是 Credential Provider 的系统入口，但不要求用户每次点击磁贴。
3. Provider 被 LogonUI 加载后即可向 Service 发起自动识别请求。
4. Service 认证通过后通知 Provider 调用 `CredentialsChanged`。
5. LogonUI 重新枚举凭据时，Provider 在 `GetCredentialCount` 返回默认凭据和自动登录标记。
6. `GetSerialization` 只在凭据材料已准备好时调用 `CredPackAuthenticationBufferW`。
7. PIN / 密码 / Windows 原登录方式必须保留为 fallback。

任务：

1. `windows_provider` 设置 crate-type 为 `cdylib`。
2. 导出：
   - `DllGetClassObject`
   - `DllCanUnloadNow`
   - `DllMain`
3. 实现 `IClassFactory`。
4. 实现 `ICredentialProvider`。
5. 实现 `ICredentialProviderCredential`。
6. 实现可显示/可隐藏的基础凭据磁贴。
7. 实现 `Advise` / `UnAdvise` 生命周期。
8. 实现 Provider 加载后的自动 wake request，键鼠触发只作为额外唤醒源。
9. 与 Service 通过 named pipe 通信。
10. 认证通过后调用 `CredentialsChanged`。
11. 在 `GetCredentialCount` 中区分普通展示状态和自动登录准备完成状态：
   - 未准备好时可显示磁贴或隐藏磁贴。
   - 凭据准备好时返回 `pdwdefault = 0`。
   - 凭据准备好时返回 `pbautologonwithdefault = TRUE`。
12. 在 `GetSerialization` 中调用 `CredPackAuthenticationBufferW`。
13. 实现 `ReportResult`，避免密码错误或认证失败时无限重试。
14. 实现安装和卸载脚本。

验收：

1. 只在虚拟机测试。
2. Provider 可注册。
3. Provider 可卸载。
4. Provider 崩溃后可恢复系统登录。
5. 锁屏或登录界面加载后，不点击磁贴也能唤醒 Service 进行识别。
6. 认证成功后自动登录或自动解锁。
7. 用户仍可手动选择 PIN / 密码 fallback。
8. 密码错误不会无限重试。
9. 磁贴隐藏时，认证成功后仍能通过自动登录凭据完成提交。
10. 卸载后 Windows 原登录方式恢复。

## 十、Phase 6：开机未登录自动登录强化

目标：在 Phase 5 自动登录链路稳定后，强化冷启动、无人登录、资源权限和多账户策略。

进入 Phase 6 前增加 Phase 5.5：Face Auth 可观测性、校准与模型路线复评。原因是 Phase 6 会把真实人脸识别放到更敏感的“开机未登录 / LogonUI 自动登录”场景中，如果当前识别模型、阈值、摄像头分辨率和头部姿态边界没有量化，后续问题会混在 Windows 登录生命周期里难以定位。

Phase 5.5 目标：

1. 给人脸检测和识别链路增加可视化标注输出。
2. 统计检测成功率、匹配分数分布、连续成功策略和耗时。
3. 建立正脸、左右偏头、低头抬头、背光、低像素摄像头等场景 sweep。
4. 复评 YuNet + SFace 是否继续作为 baseline。
5. 保持 detector 和 recognizer 独立热插拔。

详见：

```text
docs/PHASE5_5_FACE_AUTH_CALIBRATION.md
```

任务：

1. Service 设置为开机非延迟自动启动，避免 LogonUI 已加载但 Service 仍未就绪。
2. 验证 LocalSystem 下可访问 Credential Store。
3. 验证登录界面可使用本地摄像头。
4. 验证模型资源路径在未登录时可读。
5. 验证 Provider 与 Service 的会话绑定；Provider 在 Service 尚未就绪时必须后台重试，而不是直接放弃本次开机自动登录。
6. 实现多账户选择策略。
7. 实现失败后回退手动密码。
8. 实现开机后首次必须手动密码的可选策略。

验收：

1. 重启后未登录状态下 Service 已运行。
2. 登录界面加载后可自动开始识别。
3. 键鼠触发可作为额外唤醒和重试入口。
4. 认证成功能登录目标账户。
5. 认证失败不影响手动密码或 PIN 登录。
6. 可安全卸载恢复。

### Phase 6.5：Presence Lock 离座自动锁屏

目标：用户已登录桌面后，低频检测当前用户是否仍在电脑前；连续无脸或连续检测到非本人时自动锁屏。

这个阶段不属于登录认证链路，不触碰 Credential Provider、Credential Store、Windows 密码或 `AuthGrant`。它只在已登录桌面阶段运行，通过独立的 `presence_monitor`、`presence_policy`、`presence_audit`、`session_lock` 和 `camera_lease` 模块实现。

默认策略：

1. 检测到本人时，采样间隔从 10 秒逐步拉长到 30 秒和 60 秒。
2. 连续 3 次无脸时自动锁屏，无脸检测间隔为 10 秒。
3. 首次检测到人脸但低于 Presence 阈值时立即保存本地审计记录。
4. 屏幕截图审计使用独立开关，默认开启；首次未知人脸低匹配时保存一次当前屏幕截图，用户可显式关闭。
5. 进入未知人脸怀疑状态后，按 1 秒间隔检测；连续 3 次低匹配时自动锁屏。
6. 摄像头不可用时不锁屏，避免和视频会议或诊断工具冲突。
7. 第一版默认关闭，通过 CLI 和配置显式开启。

详见：

```text
docs/PHASE6_5_PRESENCE_LOCK.md
```

## 十一、Phase 7：安装、卸载和恢复

目标：避免系统进不去。

任务：

1. `installer_cli install`：
   - 安装 Service。
   - 注册 Credential Provider。
   - 写注册表配置。
   - 设置资源目录 ACL。
2. `installer_cli uninstall`：
   - 注销 Credential Provider。
   - 停止并删除 Service。
   - 保留或删除数据按参数选择。
3. `installer_cli repair`：
   - 检查 Provider 注册表。
   - 检查 Service 状态。
   - 检查 ACL。
4. `installer_cli emergency-disable`：
   - 只禁用 Provider。
   - 不删除用户数据。

验收：

1. 安装全程有日志。
2. 卸载后 Windows 原登录方式恢复。
3. 紧急禁用可在安全模式或管理员命令行执行。
4. 所有注册表路径、服务名、管道名使用 WinFaceUnlock 自有命名。

## 十二、Phase 8：配置入口

目标：提供用户可操作入口。

优先顺序：

1. Rust CLI。
2. 后续 Tauri + TypeScript/Vue 配置 UI。
3. 再后续接入 `PC_Client` 控制中心。

第一版 CLI 命令：

```text
winfaceunlock enroll-user
winfaceunlock enroll-face
winfaceunlock list-users
winfaceunlock disable-user
winfaceunlock set-policy
winfaceunlock status
winfaceunlock test-camera
winfaceunlock test-face
winfaceunlock install
winfaceunlock uninstall
winfaceunlock emergency-disable
```

验收：

1. 不打开 UI 也能完成初始化。
2. 能注册账户和人脸。
3. 能修改阈值和冷却策略。
4. 能查看脱敏日志。

## 十三、Phase 9：小车增强

目标：不影响基础本机解锁的前提下扩展小车能力。

任务：

1. `video_provider` 新增 `vehicle_camera`。
2. `presence_provider` 新增 `radar_24g`。
3. 雷达只作为 wake trigger。
4. 小车视频只作为 frame provider。
5. 小车离线自动回到本地摄像头。
6. 小车端不保存密码、不发认证通过 token。

验收：

1. 小车不启动时，WinFaceUnlock 仍可工作。
2. 小车在线时，可选择小车视频源。
3. 雷达有人时可额外唤醒识别。
4. 远程信号不能直接解锁。

## 十四、开发顺序总表

```text
0. 安装 Rust/MSVC/CMake/vcpkg 环境
1. Rust workspace
2. Credential Store
3. IPC + grant
4. Windows Service
5. 本地摄像头 + Face Engine
6. VM Credential Provider
7. 开机未登录登录
8. 安装/卸载/恢复
9. CLI 配置入口
10. Tauri / PC_Client 配置入口
11. 小车视频源和雷达触发
```

任何阶段失败时，不向下一阶段推进。

## 十五、硬性验收标准

1. 核心实现必须是 Rust。
2. 不使用 Python。
3. Windows 密码不明文落 SQLite。
4. SQLCipher key 不明文落盘。
5. named pipe 必须设置 ACL。
6. Credential Provider 只在虚拟机测试到稳定后再上真机。
7. 卸载和 emergency-disable 先于真机测试。
8. 小车能力最后接入。
