# Phase 6.5 Presence Lock 离座自动锁屏

## 背景

Phase 5 已经验证 Credential Provider 可以在 LogonUI 中自动唤醒 Service 并提交凭据。Phase 6 继续强化开机未登录、冷启动、资源权限和多账户策略。

Presence Lock 是另一条独立链路：用户已经登录桌面后，低频检测当前用户是否仍在电脑前。如果检测到连续无人脸，或检测到有人脸但长期不是当前用户，则自动锁屏。

这个功能不是登录认证，不负责提交密码，不签发 `AuthGrant`，不读取 Credential Store，也不参与 Credential Provider。它只做桌面登录后的安全保持和离座锁屏。

## 目标

1. 用户离开座位后自动锁屏。
2. 陌生人靠近并看向屏幕时更快锁屏。
3. 不长期高频占用摄像头、CPU 和模型运行时。
4. 不污染登录解锁主链路。
5. 保留可配置开关和审计记录，便于调试误判。

## 非目标

1. 不替代 Windows 原生 PIN、密码或 Windows Hello。
2. 不在未登录或 LogonUI 阶段运行 Presence Monitor。
3. 不做持续视频录制。
4. 不把未知人脸上传或提交到外部服务。
5. 不用 Presence Lock 的低阈值结果放行登录。

## 参考方案

Windows 11 Presence Sensing / Lock on leave 的核心思路是检测用户在场状态变化，并在用户离开后关闭屏幕或锁定设备。它强调状态机、防抖、超时和场景例外，而不是持续高频认证。

相关参考：

- Microsoft Lock on leave: `https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/sensors-presence-lock-on-leave`
- Microsoft Presence Sensing settings: `https://support.microsoft.com/en-us/windows/managing-presence-sensing-settings-in-windows-11-82285c93-440c-4e15-9081-c9e38c1290bb`
- HP Auto Lock and Awake: `https://support.hp.com/us-en/document/ish_5824936-5824987-16`

本项目没有专用人体存在传感器，第一版使用普通摄像头和现有人脸检测/识别模块实现类似能力。

## 模块边界

建议新增以下模块：

```text
win_service
  login_auth_runtime
    负责现有 LogonUI 自动登录链路。

  presence_monitor
    负责桌面登录后的离座自动锁屏状态机。

  presence_policy
    负责采样间隔、连续失败次数、阈值和锁屏判定。

  presence_audit
    负责未知人脸事件的本地审计保存和清理，包括人脸裁剪图和可选屏幕截图。

  session_lock
    负责调用 Windows LockWorkStation。

  camera_lease
    负责摄像头占用仲裁，避免和登录识别、诊断工具或视频会议冲突。

  presence_helper
    定义用户会话 helper 请求/响应协议。Service 通过这个边界请求当前登录用户会话完成屏幕截图；diagnostics 可使用本进程 helper 实现做本地验证。
```

Credential Provider 不依赖 `presence_monitor`。Provider 只负责登录界面唤醒、自动登录凭据提交和失败后 fallback。

## 数据流

```text
用户已登录桌面
-> presence_monitor 根据策略等待下一次采样
-> camera_lease 尝试获取摄像头
-> 采集少量帧
-> 人脸检测
-> 可选本人低阈值匹配
-> presence_policy 更新状态
-> 达到锁屏条件时调用 session_lock
-> 可选写入 presence_audit
```

屏幕截图审计的调用方向：

```text
presence_monitor
-> presence_audit 创建未知人脸审计事件
-> presence_helper 请求当前用户会话保存屏幕截图
-> presence_audit 写入 screen_snapshot_path
```

`presence_helper` 第一版接口：

```text
CaptureScreenSnapshot {
  event_id,
  output_path
}

ScreenSnapshotCaptured {
  event_id,
  image_path,
  width,
  height
}

ScreenSnapshotUnavailable {
  event_id,
  reason
}
```

## 状态机

```text
Disabled
  配置关闭，不启动检测。

WaitingForDesktopSession
  等待用户登录桌面。

StableOwnerPresent
  稳定检测到当前用户，采样间隔逐步拉长。

NoFaceSuspect
  连续检测不到人脸，进入离座怀疑状态。

UnknownFaceSuspect
  检测到人脸，但匹配分低于 Presence 阈值，进入陌生人怀疑状态。

LockRequested
  达到锁屏条件，调用 LockWorkStation。

CameraUnavailable
  摄像头被占用或打开失败，本轮不锁屏，等待下一轮。
```

## 采样策略

### 稳定本人在场

Presence Lock 不应固定高频运行。建议使用递增间隔：

```text
首次检测：10 秒后
第一次检测到本人：下一次 30 秒后
第二次及以后检测到本人：下一次 60 秒后
最大稳定间隔：60 秒
```

### 连续无脸

如果检测不到人脸，进入 `NoFaceSuspect`：

```text
检测间隔：10 秒
锁屏条件：连续 3 次无脸
动作：调用 LockWorkStation
```

第一版不要求结合 Windows 键鼠空闲时间。摄像头判断连续无脸即可触发自动锁屏。

### 检测到非本人

如果检测到人脸，但本人匹配分低于 `presence_owner_match_threshold`，进入 `UnknownFaceSuspect`：

```text
首次低匹配：立即保存未知人脸审计记录
检测间隔：1 秒
锁屏条件：连续 3 次低匹配
动作：
  1. 调用 LockWorkStation。
  2. 补写最终锁屏事件元数据。
```

陌生人怀疑状态比无脸状态更敏感，因此采样频率更高。不能等连续 3 次低匹配后才保存截图，否则陌生人已经看了几秒甚至十几秒，审计证据可能丢失。

## 阈值策略

登录解锁和离座保持必须使用不同阈值。

```text
unlock_match_threshold
  用途：允许登录或解锁。
  建议值：0.75 左右。
  含义：身份认证通过。

presence_owner_match_threshold
  用途：判断当前屏幕前的人是否大概率仍是当前用户。
  建议值：0.45 到 0.55。
  含义：不锁屏的保持条件，不得用于登录放行。
```

Presence 阈值可以低一些，因为它不是授权登录，只是避免误锁。低阈值匹配失败连续出现时，才触发自动锁屏。

## 审计策略

第一次进入 `UnknownFaceSuspect` 时立即保存审计记录。无脸离座不保存图片。

如果后续连续 3 次低匹配触发锁屏，则在同一条审计记录上补写最终锁屏事件元数据。这样既能快速保留证据，也避免每 1 秒保存一张图片导致磁盘和隐私成本过高。

屏幕截图属于更敏感的审计材料，因为它可能包含聊天、文档、代码、网页、邮件、密码框或其它私密信息。项目目标是让用户回来后能确认陌生人当时可能看到了什么内容，因此 Presence Lock 启用后，屏幕截图审计默认开启。它仍必须使用独立开关控制，便于用户在不需要时关闭。第一次进入 `UnknownFaceSuspect` 时保存一次当前屏幕截图，不能做连续屏幕录制。

建议保存内容：

```text
event_id
captured_at_unix_ms
decision: unknown_face_lock_requested
match_score
presence_owner_match_threshold
face_crop_path
optional_frame_thumbnail_path
optional_screen_snapshot_path
```

默认只保存裁剪后的人脸图，不保存完整摄像头画面。完整帧缩略图必须由配置显式开启。

屏幕截图保存策略：

```text
presence_audit_save_screen_snapshot = true
  默认值。第一次进入 UnknownFaceSuspect 时额外保存当前屏幕截图。
  同一轮未知人脸事件只保存一次屏幕截图。
  不连续录屏，不周期性截图。

presence_audit_save_screen_snapshot = false
  关闭屏幕截图审计。未知人脸事件只保存人脸裁剪图。
```

屏幕截图默认开启只在 Presence Lock 已启用时生效。如果 `presence_lock_enabled = false`，不会进行摄像头检测，也不会保存人脸裁剪图或屏幕截图。

存储目录：

```text
C:\ProgramData\WinFaceUnlock\presence-audit\
```

安全要求：

1. ACL 只允许 LocalSystem、Administrators 和必要的当前用户读取。
2. 默认最多保留最近 20 到 50 条记录。
3. 记录不得进入 git。
4. 日志只记录路径、分数和事件类型，不记录密码、token 或 Credential Store 内容。
5. 提供 CLI 清理命令，例如 `clear-presence-audit`。
6. 屏幕截图审计必须使用独立开关，默认开启，但可以由用户关闭。

## 摄像头占用策略

Presence Monitor 必须通过 `camera_lease` 获取摄像头使用权。

如果摄像头被视频会议、诊断工具或登录认证链路占用：

```text
本轮状态：CameraUnavailable
动作：不锁屏
下一轮：按当前状态的检测间隔重试
日志：记录 camera_unavailable，不记录为 no_face 或 unknown_face
```

摄像头不可用不能直接触发锁屏，否则会和视频会议、摄像头驱动异常或其它程序冲突。

## 和现有链路的关系

### 登录自动解锁链路

```text
LogonUI
-> windows_provider.dll
-> WinFaceUnlockService WakeAuth
-> 摄像头认证
-> Credential Store
-> 自动提交凭据
```

### Presence Lock 链路

```text
已登录桌面
-> WinFaceUnlockService presence_monitor
-> 低频摄像头检测
-> Presence 状态机
-> LockWorkStation
```

两条链路共享底层人脸检测/识别模块，但不共享授权语义。

## 配置项

建议配置字段使用明确语义命名：

```text
presence_lock_enabled
presence_stable_initial_interval_ms
presence_stable_second_interval_ms
presence_stable_max_interval_ms
presence_no_face_suspect_interval_ms
presence_unknown_face_suspect_interval_ms
presence_no_face_required_count
presence_unknown_face_required_count
presence_owner_match_threshold
presence_audit_enabled
presence_audit_save_full_frame_thumbnail
presence_audit_save_screen_snapshot
presence_audit_max_record_count
```

避免使用 `enabled`、`ok`、`success` 这类跨层含义不清的裸字段。

## CLI 诊断命令

第一版实现前应先提供诊断命令：

```powershell
diagnostics_cli.exe presence-check-once `
  --camera-id opencv-index:0 `
  --template .\face-enrollment\selected_templates.json `
  --threshold 0.50
```

输出一次检测结果：

```text
presence_frame_captured: true
single_face_detected: true
owner_match_score: 0.62
presence_owner_match_passed: true
presence_decision: OwnerPresent
```

再提供本地模拟状态机命令：

```powershell
diagnostics_cli.exe presence-policy-simulate `
  --events owner,owner,no-face,no-face,no-face
```

用于在不锁屏的情况下验证状态转换。

## 实施顺序

1. 文档和协议字段确认。已完成。
2. 新增 `presence_policy`，先写纯状态机单元测试。已完成。
3. 新增 `presence-check-once` 诊断命令，复用现有人脸检测/识别模块。已完成。
4. 新增 `presence_audit`，保存未知人脸裁剪图；屏幕截图作为独立可选项，并补保存数量上限测试。已完成。
5. 新增 `session_lock` adapter，隔离 `LockWorkStation` 平台 API。已完成。
6. 新增 `screen_snapshot` 和 `presence_helper`，先在 diagnostics 用户会话中验证屏幕截图，再为后续 helper 进程预留协议边界。已完成。
7. 新增 `presence_monitor`，在控制台/诊断模式先跑，不接 Windows Service 自动运行。已完成。
8. 实现真实摄像头 observation source，把 `presence_monitor` 接到低频检测链路。已完成 diagnostics 调试入口。
9. 实现用户会话 helper 进程或等价启动方式，解决 Service Session 0 不能稳定截图当前桌面的边界。待实现。
10. 将 `presence_monitor` 接入 `WinFaceUnlockService` session-change 后台链路。已完成：Service 接受 `SESSION_CHANGE` 控制事件，`SessionLogon`/`SessionUnlock` 启动 monitor，`SessionLock`/`SessionLogoff`/断开/终止停止 monitor。
11. 接入 `win_service` 配置开关。已完成：配置缺失时不启动；`installer_cli configure-service-auth` 默认写入 `PresenceLockEnabled=true`，可用 `--disable-presence-lock` 显式关闭。
12. 虚拟机验证自动锁屏行为。待实现。

## 验收标准

1. 配置缺失时 Presence Lock 关闭，不影响现有登录链路；完成本地摄像头认证配置后，installer 默认启用 Presence Lock。
2. 打开后，稳定检测到本人时采样间隔能从 10 秒拉长到 30 秒和 60 秒。
3. 连续 3 次无脸后触发锁屏。
4. 首次检测到未知人脸低匹配时立即保存裁剪人脸审计记录。
5. 默认开启屏幕截图审计；首次检测到未知人脸低匹配时同时保存一次当前屏幕截图，除非用户显式关闭。
6. 进入未知人脸怀疑状态后，按 1 秒间隔检测；连续 3 次低于 Presence 阈值后触发锁屏。
7. 摄像头不可用不触发锁屏。
8. 锁屏 API 封装在平台 adapter 中，不泄漏到策略模块。
9. `presence_policy` 可以单独单元测试。
10. Presence Lock 不读取 Windows 密码，不访问 Credential Store，不签发 grant。
11. `presence_monitor` 可以用模拟 observation 序列验证完整循环：未知人脸三连必须只请求一次审计并请求一次锁屏。
12. `presence-monitor-camera-debug` 可以用真实摄像头跑有限次数监控循环；调试命令必须使用模拟 locker，不得真的锁屏。
13. Service 后台链路只在 Windows session 登录或解锁后启动，锁定、注销、断开或服务停止时必须请求 monitor 退出。

## 风险和缓解

### 误锁屏

风险：用户低头、侧身或摄像头短暂检测失败导致误锁。

缓解：必须连续 3 次失败才锁屏；稳定本人状态使用低频检测；Presence 阈值低于登录阈值。

### 摄像头冲突

风险：视频会议或其它程序占用摄像头。

缓解：通过 `camera_lease` 管理；摄像头不可用进入 `CameraUnavailable`，不直接锁屏。

### 隐私风险

风险：未知人脸审计图片保存过多、屏幕截图包含敏感内容，或文件权限过宽。

缓解：屏幕截图独立开关且可关闭；同一轮未知人脸事件只保存一次屏幕截图；限制保留数量；设置 ProgramData ACL；提供清理命令。

### 用户会话截图边界

风险：`win_service` 以 Windows Service 运行时处于 Session 0。登录后的用户桌面在交互式用户会话中，Service 直接调用屏幕截图 API 可能只能截到空白、锁屏隔离桌面或 Session 0 内容，不能稳定拿到用户当前桌面。

缓解：第一版先在 diagnostics 用户进程中验证屏幕截图和审计格式。真正接入常驻 Presence Monitor 时，屏幕截图能力应通过 `presence_helper` 完成。helper 可以是用户会话后台进程、计划任务、托盘进程或后续配置 UI 的后台部分。Service 只负责策略编排、审计目录和锁屏请求，不假设自己能直接读取用户桌面像素。

### 登录链路污染

风险：Presence Lock 和 Credential Provider 共享状态，导致登录行为不可预测。

缓解：Provider 不依赖 Presence Monitor；Presence Lock 不触碰凭据和 grant；只共享底层检测/识别 provider。
