# WinFaceUnlock 实施方案与阶段计划

更新时间：2026-05-31

## 一、项目定位

WinFaceUnlock 是一个独立的 Windows 人脸解锁外挂项目，后续目录固定为：

```text
D:\study\workspace\python_workspace\WinFaceUnlock
```

它和 `PC_Client` 的关系是联动，而不是内嵌：

1. `PC_Client` 负责现有控制中心、媒体源配置、小车状态展示和用户设置入口。
2. `WinFaceUnlock` 负责人脸认证与 Windows 解锁。
3. 两者通过本机 IPC / HTTP / WebSocket 等明确 contract 通信。
4. WinFaceUnlock 必须可以脱离 `PC_Client` 和小车端独立运行。

这个边界的原因是：Windows 解锁涉及 Credential Provider、Winlogon、凭据序列化和本地高权限组件，风险高，不应混入现有 Python 主服务。

## 二、当前讨论结论

### 2.1 FaceWinUnlock-Tauri 的参考价值

FaceWinUnlock-Tauri 当前公开主分支已经删除大量核心代码，但旧分支仍能看到关键技术路线：

1. Credential Provider DLL 实现 `ICredentialProvider`。
2. 通过命名管道接收用户名和密码。
3. `CredentialsChanged` 通知 Windows 凭据状态变化。
4. `GetSerialization` 使用 `CredPackAuthenticationBufferW` 打包凭据。
5. OpenCV `FaceDetectorYN` / `FaceRecognizerSF` 用于人脸检测和识别。

可参考的部分：

1. Windows Credential Provider 的工程结构。
2. Winlogon 场景下的生命周期和事件触发方式。
3. 命名管道通信的基本形态。
4. YuNet + SFace 的轻量本地识别链路。

不能直接照搬的部分：

1. 公开源码不完整，主分支核心代码已被删除。
2. 旧分支存在明文用户名密码通过管道传递的问题。
3. 其默认摄像头是本机 OpenCV `VideoCapture`，不适配我们的小车视频源主线。
4. 其安全等级不能等同 Windows Hello。
5. AGPL-3.0 许可证要求需要注意；若复制代码会触发开源义务。本项目应优先独立实现，只参考公开机制与思路。

### 2.2 模型链路判断

FaceWinUnlock-Tauri 使用的公开模型链路大致是：

```text
摄像头帧
-> YuNet: face_detection_yunet_2023mar.onnx
-> SFace: face_recognition_sface_2021dec.onnx
-> 余弦相似度匹配
-> 可选 RGB 活体检测模型
```

含义：

1. 人脸检测是人脸识别的前置条件。
2. 检测负责找脸、定位人脸框和关键点。
3. 识别负责对齐人脸、提取特征、与注册模板比对。
4. 视频质量、颜色、曝光、角度和帧率会先影响检测，再影响识别。

对本项目的建议：

1. 第一版可采用 YuNet + SFace，原因是轻量、部署简单、OpenCV 支持成熟。
2. 后续再评估 InsightFace / ONNX Runtime / GPU 推理等路线。
3. 模型 provider 必须做成可替换，不要把 OpenCV 细节写死到上层解锁流程。

## 三、路线选择

### 3.1 被淘汰路线：小车端认证后回传凭证

```text
小车摄像头
-> 小车端做人脸识别
-> 小车回传“认证通过”
-> PC 解锁
```

不推荐原因：

1. 小车和网络链路会变成认证安全边界。
2. “认证通过”信号如果被伪造或重放，PC 端很难判断。
3. Windows 凭据和解锁策略仍在 PC，本质上无法完全信任远端普通设备。
4. 要让小车成为可信认证端，需要双向 TLS、私钥保护、nonce、防重放、远程设备身份绑定，成本明显升高。

结论：不作为主线。

### 3.2 推荐主线：PC 端独立运行，小车端作为可选增强

```text
本地摄像头/可选小车摄像头
-> PC 统一视频帧源
-> FaceAuthEngine
-> UnlockPolicy
-> UnlockBroker
-> Credential Provider
-> Windows 解锁
```

基础独立模式：

1. Windows 锁屏后监听鼠标或键盘输入。
2. 输入事件唤醒本机 FaceUnlockAgent。
3. 从本地摄像头取帧并完成认证。
4. 小车端未运行、未连接或完全不存在时，解锁能力仍然可用。

小车端可选职责：

1. 提供视频帧。
2. 提供 24G 毫米波雷达人体存在状态。
3. 提供连接、健康、延迟等状态。

PC 端职责：

1. 决定当前 active video provider。
2. 完成人脸检测、人脸识别和活体/挑战校验。
3. 生成短时本机授权。
4. 执行 Windows 解锁。
5. 记录日志和失败原因。

这个方案的优势：

1. 基础能力不依赖小车端，项目可以独立安装、独立验证和独立使用。
2. 安全边界留在 PC。
3. 小车 24G 雷达接入后可以进一步减少无效模型加载和视频拉流。
4. 小车离线时无需切换到特殊兜底路径，本机模式本来就是有效主路径。
4. 和 `PC_Client` 现有“统一视频源”方向一致。

## 四、目标架构

### 4.1 独立项目模块

建议 WinFaceUnlock 内部按以下模块拆分：

```text
WinFaceUnlock/
  README.md
  docs/
  agent/
    face_auth/
    video/
    presence/
    policy/
    broker/
    ipc/
  windows_provider/
  config/
  tests/
```

说明：

1. `agent/face_auth`：人脸检测、特征提取、模板比对、活体/挑战校验。
2. `agent/video`：统一视频帧 provider，支持小车视频源和本地摄像头。
3. `agent/presence`：小车 24G 雷达人体存在 provider。
4. `agent/policy`：何时唤醒、何时冷却、失败次数、锁屏状态、fallback 策略。
5. `agent/broker`：本机短时授权和 Windows 解锁请求边界。
6. `agent/ipc`：与 `PC_Client` 和 Credential Provider 的通信。
7. `windows_provider`：Credential Provider DLL / 安装卸载脚本 / 注册表配置。
8. `config`：默认配置、用户配置 schema、示例文件。
9. `tests`：协议、策略、模型 provider、凭证有效性测试。

### 4.2 与 PC_Client 的联动

`PC_Client` 不直接做人脸解锁，只提供设置和状态入口：

1. 启用/禁用 WinFaceUnlock。
2. 选择视频源：小车视频源 / 本地摄像头。
3. 展示小车在线、雷达有人、识别中、认证成功、失败原因。
4. 配置阈值、冷却时间、失败次数上限。
5. 查看解锁日志。

通信建议：

```text
PC_Client -> WinFaceUnlock:
- update_config
- select_video_source
- enable_unlock
- disable_unlock
- request_status

WinFaceUnlock -> PC_Client:
- agent_status
- presence_changed
- auth_state_changed
- unlock_attempt_logged
- error_reported
```

### 4.3 基础触发与小车增强策略

基础独立模式：

```text
Windows 锁屏
-> 鼠标或键盘输入
-> 唤醒 FaceUnlockAgent
-> warmup model/video
-> authenticating
-> unlock_success / unlock_failed
-> cooldown
-> 等待下一次输入
```

小车在线时：

```text
radar_present
-> warmup model/video
-> authenticating
-> unlock_success / unlock_failed
-> cooldown
-> idle
```

注意：雷达是新增触发源，不是基础依赖。推荐始终只常驻轻量 supervisor，模型和摄像头按需加载。

## 五、凭证有效性策略

第一版先不展开加密实现，但必须先定义有效性原则。

不要设计成“小车发 token，PC 直接解锁”。推荐由 PC 本机 FaceUnlockAgent 生成一次性授权：

```json
{
  "subject": "windows_user",
  "source": "vehicle_camera",
  "match_score": 0.82,
  "liveness_score": 0.71,
  "issued_at": "2026-05-31T11:20:00+08:00",
  "expires_at": "2026-05-31T11:20:05+08:00",
  "session_id": "current-lock-session",
  "nonce": "single-use-random"
}
```

UnlockBroker 只接受满足以下条件的授权：

1. 本机 FaceAuthEngine 产生。
2. 有效期极短，建议 3 到 5 秒。
3. 绑定当前锁屏会话。
4. 绑定一次性 nonce。
5. 解锁一次后立即失效。
6. 失败次数超过阈值后进入冷却。
7. 日志不记录明文密码、token、完整人脸图像。

## 六、安全原则

第一版安全定位：

1. 这是便捷解锁，不等同 Windows Hello。
2. 普通 RGB 2D 人脸识别可能被照片、视频、屏幕重放攻击。
3. 小车 24G 雷达只能作为存在触发，不是认证依据。
4. 任何远程信号都不能直接触发解锁。
5. Windows 密码如果必须保存，应后续使用 DPAPI / Windows Credential Manager / 本机受保护存储，不允许明文落库。
6. Credential Provider 和 UnlockBroker 通信必须限制本机访问权限，不能开放网络入口。
7. 建议先在虚拟机验证 Credential Provider，避免登录界面异常导致无法进入系统。

## 七、实施阶段

### Phase 0：项目骨架和技术验证

目标：证明独立 agent 可以跑通最小闭环，不接 Windows 解锁。

任务：

1. 创建 WinFaceUnlock 项目骨架。
2. 定义配置 schema。
3. 实现 `VideoFrameProvider` 抽象。
4. 实现本地摄像头 provider。
5. 用 YuNet + SFace 跑通本地图片/视频帧识别。
6. 输出识别日志和分数，不执行解锁。

验收：

1. 能加载模型。
2. 能从至少一个视频源取帧。
3. 能检测人脸并提取特征。
4. 能注册一张人脸模板并完成相似度比对。

### Phase 1：独立触发与本机解锁闭环

目标：不依赖 `PC_Client` 和小车端，通过锁屏后的鼠标或键盘输入唤醒识别并完成本机闭环。

任务：

1. 监听 Windows 锁屏场景。
2. 接入鼠标或键盘输入触发。
3. 实现状态机：`idle / warmup / authenticating / cooldown`。
4. 实现模型按需加载和释放。
5. 增加失败次数、冷却时间和超时。
6. 用本地摄像头验证独立运行。

验收：

1. 小车端完全不运行时，WinFaceUnlock 仍然可以正常工作。
2. 锁屏后发生鼠标或键盘输入时，能够拉起识别。
3. 空闲时不持续拉视频和跑识别。
4. 日志能说明每次状态切换原因。

### Phase 2：PC_Client 联动配置

目标：让现有控制中心能管理 WinFaceUnlock，但不承载其核心逻辑。

任务：

1. WinFaceUnlock 暴露本机状态接口。
2. PC_Client 增加配置入口。
3. PC_Client 展示 agent 状态、视频源、雷达状态和识别状态。
4. PC_Client 下发启用/禁用、阈值、fallback 策略。
5. WinFaceUnlock 持久化配置并校验 schema。

验收：

1. PC_Client 可以启停 WinFaceUnlock。
2. 配置重启后仍生效。
3. 状态展示与 agent 实际状态一致。

### Phase 3：UnlockBroker 和 Credential Provider PoC

目标：在虚拟机中跑通 Windows 锁屏后的最小解锁链路。

任务：

1. 建立 `UnlockBroker` 本机 IPC。
2. 实现短时授权对象。
3. 实现 Credential Provider PoC。
4. 实现安装、注册、卸载脚本。
5. 在虚拟机中验证锁屏后解锁。
6. 失败时可安全卸载恢复。

验收：

1. 虚拟机可注册 Credential Provider。
2. FaceAuthEngine 认证成功后能触发解锁。
3. 失败不会卡死登录界面。
4. 卸载脚本可恢复系统登录状态。

### Phase 4：安全加固

目标：减少本机凭据和 IPC 风险。

任务：

1. 管道 / IPC 加 ACL，只允许指定本机用户或服务访问。
2. 授权增加 nonce、防重放、过期检查。
3. Windows 凭据使用 DPAPI / Credential Manager 保护。
4. 日志脱敏。
5. 加入失败次数锁定和手动密码优先策略。
6. 加入开机后首次必须手动密码的可选策略。

验收：

1. 非授权本机进程不能调用解锁 IPC。
2. 旧授权无法重放。
3. 日志不泄露密码和可复用 token。

### Phase 5：小车增强、体验和可靠性优化

目标：在独立运行能力稳定后，接入小车视频源和雷达触发，并提升识别速度、稳定性和可观测性。

任务：

1. 接入小车在线状态。
2. 接入 24G 雷达人体存在状态，作为额外唤醒来源。
3. 接入小车视频源帧读取，作为可选视频 provider。
4. 模型 provider 可替换。
5. 增加活体/挑战机制：眨眼、转头、随机动作或雷达辅助判断。
6. 增加识别失败截图的可配置保存策略。
7. 增加性能指标：唤醒耗时、检测耗时、识别耗时、视频延迟。
8. 打包安装器。

验收：

1. 常规锁屏解锁体验稳定。
2. 小车在线和离线都能按策略工作。
3. 用户能看懂失败原因。

## 八、第一版建议技术栈

第一版技术栈固定为 Rust 主线，不使用 Python PoC 或 Python sidecar。

1. Agent / Service：Rust。
2. 模型：OpenCV YuNet + SFace。
3. IPC：Windows named pipe，并设置 ACL。
4. 配置：本地 TOML / SQLCipher SQLite + schema 校验，后续再接 PC_Client 配置中心。
5. Windows Provider：Rust + windows-rs。
6. Credential Store：SQLCipher + DPAPI LocalMachine / CNG / TPM protected master_key。
7. 安装、卸载、诊断工具：Rust CLI。

推荐拆分：

```text
Rust:
- WinFaceUnlockService
- CredentialStore
- FaceAuthEngine
- VideoFrameProvider
- UnlockPolicy
- UnlockBroker
- Credential Provider DLL
- 安装/卸载工具
```

后续配置 UI 可以使用 Tauri + TypeScript/Vue，但 UI 不进入解锁主链路。

## 九、近期最小可执行任务

下一步建议按这个顺序做：

1. 创建项目骨架。
2. 定义 `VideoFrameProvider`、`FaceAuthEngine`、`UnlockPolicy` 的接口。
3. 做本地摄像头 + 本地图片的人脸注册/识别 PoC。
4. 做锁屏后的鼠标/键盘输入触发状态机。
5. 在虚拟机中做 UnlockBroker + Credential Provider PoC。
6. 稳定后再接 PC_Client 配置入口。
7. 最后接小车视频源和 24G 雷达触发。

不要一开始就接小车。先证明“本地视频源 + 识别 + 本机触发策略”稳定，再进入 Windows 解锁虚拟机验证；小车视频源和雷达是增强项。

## 十、验收总标准

最终项目应该满足：

1. 不运行小车端和 `PC_Client` 时，WinFaceUnlock 仍可独立运行。
2. Windows 锁屏后，鼠标或键盘输入可以唤醒识别。
3. 小车在线时，雷达可以作为额外触发源自动唤醒识别。
4. 最终认证在 PC 本机完成。
5. 远程信号不能直接解锁。
6. Windows 解锁组件可安装、可卸载、可恢复。
7. 所有失败都有可读日志，但不泄露敏感凭据。
8. 后续可替换模型、视频源和 presence provider。
