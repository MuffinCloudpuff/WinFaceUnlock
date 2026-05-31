# WinFaceUnlock 总体技术路线

更新时间：2026-05-31

## 一、项目目标

WinFaceUnlock 是一个自研的 Windows 人脸解锁项目，目标是实现类似 FaceWinUnlock-Tauri 的本机人脸解锁能力，但源码、协议、安全边界和后续扩展都由本项目自行控制。

项目必须先满足：

1. 不依赖 `PC_Client`。
2. 不依赖小车端。
3. 能在本机独立安装、独立运行、独立卸载。
4. 支持 Windows 锁屏后通过鼠标或键盘操作唤醒识别。
5. 默认使用本地摄像头完成识别。
6. 后续可扩展小车摄像头和 24G 毫米波雷达触发。
7. 最终认证、凭据保护和 Windows 解锁都在 PC 本机完成。

小车端只作为增强能力：

1. 可选视频源。
2. 可选 presence trigger。
3. 不保存 Windows 凭据。
4. 不直接发放“可解锁”的认证结果。

## 二、编程语言与环境基线

本项目核心主线固定为 Rust，不使用 Python 作为 PoC、sidecar 或长期运行时。

### 2.1 固定语言

```text
Rust:
  - WinFaceUnlockService
  - Credential Provider DLL
  - Credential Store
  - IPC / named pipe
  - 策略状态机
  - 本地摄像头 provider
  - FaceModelProvider
  - 安装、卸载、诊断 CLI
  - 单元测试和集成测试

TypeScript / Vue:
  - 后续可选配置 UI
  - 只做前端界面，不进入解锁主链路

C / C++:
  - 只作为 OpenCV、SQLCipher 等原生依赖的编译产物
  - 不作为本项目业务代码主线

Python:
  - 不进入本项目实现路线
  - 不作为模型验证工具
  - 不作为常驻服务
  - 不作为 Credential Provider 或 Broker 的 sidecar
```

如后续确实必须引入 C++ Face Engine，需要单独记录原因、边界、C ABI 和清理计划。默认不引入。

### 2.2 开发环境

目标平台：

```text
Windows 10/11 x64
优先 Windows 11 x64
Credential Provider 测试必须在虚拟机中进行
```

Rust 工具链：

```text
rustup
stable-x86_64-pc-windows-msvc
target: x86_64-pc-windows-msvc
```

Windows 构建依赖：

```text
Visual Studio 2022 Build Tools
  - Desktop development with C++
  - MSVC x64 toolchain
  - Windows 10/11 SDK

CMake
Ninja
Git
vcpkg
```

原生库：

```text
OpenCV
  - YuNet / SFace / liveness 模型加载
  - Rust 通过 opencv crate 或封装层调用

SQLCipher
  - 加密 SQLite 数据库
  - Rust 通过 rusqlite/libsqlite3-sys/sqlcipher 构建配置接入
```

Windows API：

```text
windows-rs
  - Credential Provider COM
  - DPAPI
  - CNG / BCrypt
  - named pipe
  - registry
  - service control
  - session / lock state
```

当前本机检查结果：

```text
已发现:
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

因此正式编码前的第一个任务是安装并固定 Rust/MSVC/CMake/vcpkg 构建环境。

### 2.3 版本固定原则

仓库创建后必须固定：

```text
rust-toolchain.toml
Cargo.lock
vcpkg baseline 或依赖版本
OpenCV 模型文件名和 hash
SQLCipher 构建参数
Windows Provider CLSID
服务名
管道名
注册表路径
```

禁止在开发过程中临时切换到 Python 验证路线。模型验证也通过 Rust CLI 完成。

## 三、参考项目定位

参考项目：

```text
FaceWinUnlock-Tauri
https://github.com/zs1083339604/FaceWinUnlock-Tauri
```

该项目对本项目有参考价值，但不作为直接改造底座。

原因：

1. 当前 `main` 快照已经删除大量核心 Rust 代码。
2. `Unlock` 后台服务基本被清空。
3. `CPipeListener` 键鼠触发和管道监听被清空。
4. `GetSerialization` 和 `ReportResult` 的核心逻辑被清空。
5. 人脸识别、注册、初始化、安装卸载等关键模块被清空。
6. 旧分支存在明文密码存储和明文管道传输问题。
7. AGPL-3.0 许可证会影响直接复制源码后的发布方式。

可参考内容：

1. Rust `cdylib` 实现 Windows Credential Provider 的工程结构。
2. `DllGetClassObject` / `DllCanUnloadNow` / `DllMain` 的基本形态。
3. `ICredentialProvider` 生命周期。
4. `ICredentialProviderCredential` 的字段、磁贴和序列化入口。
5. `CredentialsChanged` 触发自动登录的思路。
6. OpenCV YuNet + SFace 模型链路。
7. Tauri 配置界面和日志界面思路。
8. 安装、卸载、注册表和计划任务的产品流程。

不可继承内容：

1. SQLite 中明文保存 `user_pwd`。
2. 管道中直接传输 `username::FaceWinUnlock::password`。
3. 未加 ACL 的本机 IPC。
4. 把复杂模型推理和视频读取塞进 Credential Provider DLL。
5. 直接复用 GUID、注册表路径、管道名和品牌标识。

## 四、威胁模型

本项目不是 Windows Hello 级别的强认证系统，而是个人设备上的便捷解锁增强。

重点防护：

1. `database.db` 被普通复制后直接看到 Windows 密码。
2. 本机普通权限恶意程序直接读取配置和数据库。
3. 未授权本机进程随意调用解锁 IPC。
4. 日志、崩溃文件、普通业务表泄露敏感凭据。
5. 远程小车信号被伪造后直接解锁 PC。

暂不作为第一优先级防护：

1. 离线拆硬盘取证。
2. 已获得管理员或 SYSTEM 权限的高级攻击者。
3. 拥有内存 dump、进程注入、驱动级能力的攻击者。
4. 国家级或商业间谍级攻击。

因此，BitLocker 不作为第一版硬要求。若后续希望抵抗离线拆盘，应再启用 BitLocker 或等价全盘加密。

## 五、总体架构

推荐架构：

```text
WinFaceUnlockService
  - Windows Service
  - LocalSystem 开机启动
  - 负责模型、摄像头、人脸认证、Credential Store、短时授权

Credential Provider DLL
  - Rust cdylib
  - 被 Winlogon / LogonUI 加载
  - 负责登录界面集成、键鼠触发、磁贴、CredentialsChanged、GetSerialization
  - 不负责模型推理和复杂业务逻辑

Credential Store
  - SQLCipher 加密 SQLite
  - DPAPI / CNG / TPM 保护 master_key
  - 单独保护 Windows 密码 blob

Config UI / PC_Client Integration
  - 后续配置入口
  - 不进入解锁主链路

Vehicle Integration
  - 后续可选视频源
  - 后续可选雷达触发源
```

基础闭环：

```text
Windows 锁屏 / 开机登录界面
-> 用户移动鼠标或按键盘
-> Credential Provider 通知 Service 开始识别
-> Service 打开本地摄像头
-> YuNet 检测人脸
-> SFace 提取特征并比对模板
-> 可选活体检测
-> 认证通过
-> Service 生成短时授权
-> Service 解密 Windows 凭据 blob
-> 通过受 ACL 限制的 named pipe 发给 Credential Provider
-> Credential Provider 调 CredPackAuthenticationBufferW
-> Windows 完成登录或解锁
-> 清理明文密码、授权和临时状态
```

小车增强闭环：

```text
小车在线
-> 24G 雷达检测到有人靠近
-> 作为额外触发源通知 Service
-> Service 可选择小车视频源
-> 其余认证和解锁流程不变
```

## 六、为什么先做后台服务

“先做后台服务，不先做 Credential Provider”的意思是：

Credential Provider DLL 会被 Windows 登录界面加载。它一旦崩溃、注册表写错、接口返回错误，可能影响锁屏或登录界面，调试成本高，必须在虚拟机中谨慎验证。

而后台服务的大部分能力可以先在普通桌面环境验证：

1. 生成 master_key。
2. 保护和解保护 master_key。
3. 打开 SQLCipher 数据库。
4. 保存和读取用户映射。
5. 加密和解密 Windows 密码 blob。
6. 加载 YuNet / SFace / 活体模型。
7. 打开本地摄像头并取帧。
8. 完成人脸注册和识别。
9. 建立 named pipe。
10. 验证 ACL、nonce、过期时间和一次性授权。

这些能力稳定后，再把 Credential Provider 接上。这样可以把最危险的 Winlogon 调试阶段压缩到最小。

## 七、加密与凭据保护方案

### 7.1 不采用的方案

不采用时间戳作为密钥。

原因：

1. 时间戳不是秘密。
2. 时间戳可从文件元数据、日志、事件日志、注册表、WAL/journal 等痕迹推断。
3. 即使精度很高，只要攻击者能缩小时间窗口，搜索空间仍远低于 128 bit 或 256 bit 随机密钥。
4. 程序自己也必须能复原时间戳，否则重启、休眠、校时、恢复数据库后可能无法解密。

不采用自己写 AES 并自行保存 key 作为主方案。

原因：

1. AES 算法本身不是问题，key 管理才是问题。
2. 如果 key 写在代码、配置、注册表或普通文件里，本质只是把明文密码换成明文 key。
3. 如果 key 再交给 DPAPI / CNG / TPM 保护，真正安全边界就已经是 Windows/TPM 密钥保护系统。

### 7.2 推荐方案

推荐方案：

```text
database.db
  使用 SQLCipher 加密。

master_key
  32 字节真随机。
  通过 Windows CSPRNG 生成。
  用于打开 SQLCipher 数据库。

protected_master_key
  master_key 经 DPAPI LocalMachine / CNG machine key / TPM 包装后落盘。

credential blob
  Windows 密码单独加密保存。
  不作为普通字段明文写入 SQLite。

SQLite 普通数据
  user_sid
  username
  account_type
  credential_ref
  face_template_ref
  policy_json
  hardware_fingerprint_hash
```

初始化流程：

```text
1. Service 生成 32 字节 master_key。
2. 使用 DPAPI LocalMachine 或 CNG/TPM 保护 master_key。
3. 保存 protected_master_key。
4. 使用 master_key 初始化 SQLCipher 数据库。
5. 用户录入 Windows 用户名、密码和人脸模板。
6. Windows 密码单独加密成 credential blob。
7. SQLite 只保存 credential_ref 和非敏感映射。
```

运行流程：

```text
1. Service 开机启动。
2. Service 读取 protected_master_key。
3. 调用 Windows 密钥保护接口解出 master_key。
4. 使用 master_key 打开 SQLCipher 数据库。
5. 人脸认证通过后，短时解密对应 credential blob。
6. 明文密码只存在内存中，并尽快 zeroize。
```

### 7.3 硬件绑定

硬件绑定可以做，但作为附加校验，不作为主密钥来源。

可采集信息：

1. 机器 SID。
2. 主板 UUID。
3. 磁盘序列号。
4. TPM presence。
5. Windows 安装标识。
6. 可选的 CPU / 主板 / BIOS 信息。

用途：

1. 计算 `hardware_fingerprint_hash`。
2. 解密前检查当前硬件指纹是否与注册时一致。
3. 作为 DPAPI optional entropy 的输入之一。
4. 作为异常迁移检测。

限制：

1. 硬件信息不是秘密，本机程序通常能读取。
2. 硬件会变更，必须设计恢复流程。
3. 不应直接由硬件信息派生主密钥。

## 八、开机未登录支持

本项目目标包含开机未登录解锁。

因此第一版架构直接按机器级服务设计：

```text
WinFaceUnlockService
  运行身份：LocalSystem
  启动方式：开机自动启动
  职责：
    - 加载配置
    - 解保护 master_key
    - 打开 SQLCipher 数据库
    - 管理摄像头和模型
    - 接收 Credential Provider 唤醒请求
    - 完成人脸认证
    - 生成短时授权
    - 解密 Windows 凭据 blob

Credential Provider DLL
  运行环境：Winlogon / LogonUI
  职责：
    - 显示磁贴或隐藏磁贴
    - 监听锁屏 / 登录界面触发
    - 与 Service 通信
    - 调用 CredentialsChanged
    - 在 GetSerialization 中提交凭据
```

第一版仍需优先验证锁屏后解锁，再验证开机未登录登录。

原因：

1. 锁屏后解锁更容易调试。
2. 开机未登录涉及服务启动时机、摄像头权限、模型资源路径和 Winlogon 生命周期。
3. 两者共享同一套 Service / Broker / Credential Store 设计。

## 九、IPC 与短时授权

Credential Provider 与 Service 通过本机 IPC 通信，Windows 下优先 named pipe。

要求：

1. 管道必须设置 ACL。
2. 只允许 LocalSystem、Administrators 和指定服务 SID 访问。
3. 不开放网络端口。
4. 消息结构化，不使用字符串拼接协议。
5. 每次认证成功生成短时 grant。
6. grant 有 nonce、issued_at、expires_at、session_id。
7. grant 只能使用一次。
8. 过期或失败后立即失效。
9. 日志不得记录密码、完整 token、完整人脸图像。

示例授权对象：

```json
{
  "grant_id": "random-id",
  "subject": "windows_user",
  "session_id": "current-logon-session",
  "source": "local_camera",
  "match_score": 0.82,
  "liveness_score": 0.71,
  "issued_at": "2026-05-31T12:00:00+08:00",
  "expires_at": "2026-05-31T12:00:05+08:00",
  "nonce": "single-use-random"
}
```

## 十、模型与视频源

第一版模型：

```text
FaceDetectorYN / YuNet
  face_detection_yunet_2023mar.onnx

FaceRecognizerSF / SFace
  face_recognition_sface_2021dec.onnx

可选活体检测
  face_liveness.onnx
```

必须抽象为：

```text
FaceModelProvider
  detect(frame) -> face boxes / landmarks
  extract(frame, face) -> embedding
  compare(a, b) -> score
  liveness(frame, face) -> score

VideoFrameProvider
  local_camera
  vehicle_camera 后续扩展
```

上层策略不直接依赖 OpenCV 具体类型。

## 十一、阶段路线

### Phase 0：Credential Store 与 Service 验证

目标：不接 Winlogon，先验证安全存储和后台核心。

任务：

1. 创建 Rust workspace。
2. 建立 `common_protocol`。
3. 实现 master_key 生成。
4. 实现 DPAPI LocalMachine 保护和解保护。
5. 接入 SQLCipher 或先定义接口。
6. 实现 credential blob 加密/解密。
7. 实现硬件指纹采集和校验。
8. 实现短时 grant / nonce / 过期 / 一次性使用。
9. 实现 named pipe PoC 和 ACL。

### Phase 1：本地摄像头和人脸识别

目标：不接 Credential Provider，先完成人脸注册和识别。

任务：

1. 接入本地摄像头。
2. 加载 YuNet / SFace。
3. 实现人脸模板注册。
4. 实现人脸比对。
5. 实现连续成功策略。
6. 实现失败冷却。
7. 实现日志脱敏。

### Phase 2：Credential Provider 虚拟机 PoC

目标：在虚拟机中跑通最小 Windows 登录/解锁链路。

任务：

1. 创建 Rust `cdylib` Credential Provider。
2. 注册 CLSID 和 Provider。
3. 实现基础磁贴。
4. 实现键鼠触发。
5. 实现与 Service 的 IPC。
6. 实现 `CredentialsChanged`。
7. 实现 `GetSerialization`。
8. 实现安全卸载脚本。

### Phase 3：开机未登录登录

目标：验证服务在用户未登录前可用。

任务：

1. Service 以 LocalSystem 开机启动。
2. 验证摄像头在登录界面可用。
3. 验证模型资源路径和权限。
4. 验证用户凭据提交。
5. 验证失败恢复。
6. 验证多账户策略。

### Phase 4：配置 UI 与 PC_Client 联动

目标：提供用户可操作配置入口。

任务：

1. 注册账户。
2. 录入人脸。
3. 管理策略。
4. 查看日志。
5. 启停服务。
6. 可选接入 `PC_Client` 控制中心。

### Phase 5：小车增强

目标：接入小车视频源和 24G 雷达触发。

任务：

1. 增加 vehicle video provider。
2. 增加 radar presence provider。
3. 雷达作为额外触发源。
4. 小车视频作为可选视频源。
5. 小车断开时不影响基础本机解锁。

## 十二、总原则

1. Credential Provider DLL 尽量小。
2. 模型推理、视频读取、数据库、密钥和策略都放在 Service。
3. 小车不是认证权威。
4. SQLCipher 是数据库保护层，不是密钥管理系统。
5. master_key 必须是真随机。
6. master_key 必须由 DPAPI / CNG / TPM 保护。
7. 硬件绑定只做附加校验。
8. 明文 Windows 密码只在认证通过后的短时间内出现在内存中。
9. 所有 Winlogon 相关测试先在虚拟机完成。
10. 卸载和恢复能力必须优先设计。
