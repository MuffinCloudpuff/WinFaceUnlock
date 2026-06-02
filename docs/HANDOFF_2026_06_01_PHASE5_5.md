# WinFaceUnlock 当前交接说明

更新时间：2026-06-01

## 一句话状态

项目主线已经从 Credential Store、IPC、Windows Service、Credential Provider 虚拟机验证，推进到 Phase 5.5 的人脸识别效果优化阶段。当前重点不是 UI，而是把本机摄像头人脸注册、姿态引导、模板集、调试快照和阈值校准做成可测、可解释、可替换的后端链路。

## 当前原则

- 项目主线是 Rust workspace，不使用 Python PoC、Python sidecar 或 Python 常驻服务。
- Windows 登录保底方式不能被破坏：PIN、密码和系统原生登录必须始终保留。
- Credential Provider、LogonUI、开机未登录场景必须先在虚拟机验证，再考虑真机。
- 人脸识别只是登录方式之一，程序失败时只能导致本模块失败，不能损坏 Windows 或让用户无法登录。
- 检测模型、识别模型、位姿模型、注册流程、登录认证流程必须解耦，按需组合。

## 已完成的主线能力

### 1. Rust workspace 和基础模块

当前 workspace 主要 crate：

```text
common_protocol
credential_store
ipc
win_service
windows_provider
installer_cli
diagnostics_cli
video_provider
face_engine
face_auth
face_pose
face_pose_mediapipe
hardware_binding
```

已确认 `win_service` 依赖 `face_auth` 时关闭默认 feature：

```toml
face_auth = { path = "../face_auth", default-features = false }
```

这保证 Service 登录热路径不带入注册期的位姿、质量评分、debug 图片导出和 MediaPipe 依赖。

### 2. Credential Store / Service / IPC / Provider

已经完成并验证：

- Credential Store 的加密存储和数据库结构。
- Windows Service 安装、启动、停止、卸载。
- Named Pipe IPC 请求/响应。
- `WakeAuth`、`FetchCredential`、`FetchCredentialMaterial` 协议链路。
- Windows Credential Provider 虚拟机加载。
- LogonUI 自动唤醒和 `CredentialsChanged` 相关卡顿问题修复。
- Provider 默认使用隐藏磁贴后台识别：`TileVisibility=hidden-until-ready` 且 `AutoWakeOnAdvise=true`。认证成功后自动登录；认证失败时不选中 WinFaceUnlock 磁贴，LogonUI 保持 Windows 原生 PIN/密码路径。
- 紧急禁用 Provider 的恢复路径。

虚拟机里已经验证过：

- Provider 能被 LogonUI 加载。
- Service 能发起本地摄像头认证。
- 成功后能取回已加密保存的 Windows 凭据材料。
- PIN 和密码不被禁用。

### 3. 人脸检测与识别 baseline

当前 baseline：

```text
检测模型：OpenCV Zoo YuNet
识别模型：OpenCV Zoo SFace
推理方式：OpenCV DNN / OpenCV Face API
默认策略：CPU 跑，不依赖 GPU
```

模型文件约定放在项目目录：

```text
models/face_detection_yunet_2023mar.onnx
models/face_recognition_sface_2021dec.onnx
models/minifasnet_v2.onnx
models/face_landmarker.task
```

`models/*.onnx` 和 `models/*.task` 已加入 `.gitignore`，避免把大模型文件提交进仓库。

当前识别链路已经支持：

- 检测单张人脸。
- 对齐 crop。
- 提取 SFace embedding。
- 多模板匹配。
- 模板记录 recognizer model identity。
- recognition model 不匹配时拒绝认证。

## Phase 5.5 当前新增能力

### 0. 当前活体检测状态和卡点

当前活体检测仍限定为 `screen replay attack detection`，也就是屏幕回放攻击检测；它不是完整活体检测，也还没有接入 Windows Service 登录热路径。

截至 2026-06-01，已经完成：

- 新增 `face_liveness` crate，和 `face_engine`、`face_auth`、`win_service` 解耦。
- 新增 `diagnostics_cli liveness-screen-debug`，可采集逐帧 JSONL、汇总 JSON 和标注图。
- 判定语义已收敛为：检测到屏幕候选矩形本身不拒绝；只有整个人脸框完全落在屏幕候选区域内，或者人脸框与屏幕候选区域重叠率达到 `0.95`，才输出 `SpoofRejected`。
- 终端摘要已收敛为主要看 `spoof_rejected_frame_count` 和 `inconclusive_frame_count`，背景矩形计数只保留在 JSON 中辅助排查。
- 已新增窗口级策略：短时间窗口内任一帧出现 `SpoofRejected`，本次认证窗口整体拒绝。
- 已新增 MiniFASNet-V2 ONNX 静默活体 provider：默认读取 `models/minifasnet_v2.onnx`，输出 `2D fake / real / 3D fake` 三分类概率。该条为 2026-06-01 阶段状态；2026-06-02 已切换为 MiniFASNet 默认参与拒绝，详见下方更新。

当前卡点：

```text
屏幕回放场景可以拒绝一部分帧，但不能稳定拒绝全部帧。
一组 100 帧采样里出现过 spoof_rejected_frame_count = 26，剩余帧大多是 Inconclusive。
```

已确认这不是单纯调低阈值能解决的问题，主要有两层原因：

1. 单帧 `Inconclusive` 不能直接代表本次认证放行。当前已经新增短窗口/会话级策略：只要窗口内出现明确 `SpoofRejected`，本次认证窗口就整体拒绝，而不是逐帧独立放行。
2. 原先“整个人脸框完全落在矩形内”的几何判定过于依赖 YuNet 的人脸框和 OpenCV 轮廓框。调试数据里有多帧 `face_inside_screen_ratio` 已达到 0.96/0.99，但因为人脸检测框比屏幕轮廓略外扩，仍被算成不完全包含。当前已经补上 `face_inside_screen_ratio >= 0.95` 的拒绝规则。

推荐下一步优先级：

1. 在 `liveness-screen-debug` 汇总里增加未拒绝原因分桶，例如 `no_face`、`face_but_no_screen_rectangle`、`face_and_rectangle_but_face_not_contained`，让每次调试能直接看到漏检原因。
2. 用 MiniFASNet 诊断分数 + 屏幕几何后的新版本重新采集三类固定场景：纯真人、手机/屏幕回放、背景屏幕但真人在屏幕外。不要在这三组数据稳定前接入登录主线。
3. 如果仍然有高重叠但未拒绝的帧，再补充判断五点人脸 landmarks 是否全部位于屏幕候选区域内。

### 2026-06-02 活体主线收敛更新

- 已确认本地 `minifasnet_v2.onnx` 必须接收 80x80 BGR 原始 `0-255` 像素值。错误使用 `/255` 归一化会让真人稳定误判为 replay attack。
- 修正预处理后，真人采样中 MiniFASNet 已能稳定输出 `LiveAccepted`。
- 屏幕矩形几何规则会在真人背景中产生误拒，现默认不加载、不执行，也不进入窗口级拒绝策略。仅在显式使用 `--enable-screen-geometry-diagnostics` 时运行旧算法用于离线排查。
- MiniFASNet 已成为当前活体主线。默认 spoof 分数达到阈值就输出 `SpoofRejected`；仅在显式使用 `--minifasnet-diagnostic-only` 时降级为只记录分数。
- Service 登录 workflow 已接入 `MiniFasNetLivenessProvider`，不要把 `ScreenReplayLivenessProvider` 接入主线。
- Service 正式解锁使用动态 spoof 比例门禁：单帧 spoof 不立即终止；出现连续真人匹配候选时，如果当前 MiniFASNet 已评估帧中的 spoof 比例大于 `0.40`，才拒绝本次窗口，否则立即解锁。30 帧只是最长采样上限，不会等满 30 帧后再统一计算比例。
- Credential Provider 在人脸认证失败后最多自动重试 3 个 Service 窗口。默认隐藏磁贴模式下，第三次失败后停止自动重试，不显示 WinFaceUnlock 磁贴，保留 Windows PIN 或密码兜底。

### 1. face_pose 独立位姿模块

新增 `face_pose` crate，提供统一 trait：

```rust
FacePoseProvider::estimate_pose(frame, detected_face) -> FacePoseEstimate
```

当前 provider：

```text
LandmarkFacePoseProvider
  基于 YuNet 五点关键点粗估 yaw/pitch/roll。
  支持正脸、左右转头、低头、抬头的粗判断。
  不支持 blink。

MediaPipeFacePoseProvider
  通过 face_pose_mediapipe crate 动态加载 native DLL bridge。
  目标是使用 MediaPipe Face Landmarker 做更稳定的姿态和眨眼判断。
  当前 Rust adapter 和 ABI 已写好，但官方 MediaPipe Windows Bazel 构建仍是阻塞点。
```

重要边界：

- 位姿只用于注册、质量诊断和后续活体辅助。
- 不作为身份识别 embedding 模型。
- 不进入 `win_service` 默认登录热路径。

### 2. guided-enroll 手机式引导注册状态机

`diagnostics_cli guided-enroll` 已从“按时间硬切步骤”改为状态机：

```text
进入某一步
-> waiting_for_pose：持续检测姿态
-> 连续 N 帧姿态和质量达标
-> recording_started：开始保存该步骤候选样本
-> 录制阶段继续校验姿态，偏离姿态的帧不计数
-> 采够 accepted_frames_per_step
-> 自动进入下一步
```

默认步骤：

```text
1. 正脸
2. 左转
3. 右转
4. 低头
5. 抬头
6. 眨眼
```

当前默认 `landmark` provider 不支持眨眼，所以眨眼步骤会被能力过滤跳过。等 MediaPipe provider 可用后，同一套流程可以进入 blink。

推荐当前测试命令：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment `
  --accepted-frames-per-step 6 `
  --max-wait-frames-per-step 300 `
  --max-frames-per-step 180 `
  --pose-ready-consecutive 3 `
  --pose-ready-min-fit 0.25 `
  --frame-delay-ms 60 `
  --threshold 0.75 `
  --save-debug-images
```

输出：

```text
face-enrollment/
  selected_templates.json
  enrollment_report.json
  debug_frames/
  aligned_faces/
```

`face-enrollment/` 已加入 `.gitignore`，因为里面可能包含可识别人脸图片。

调试时可显式增加：

```powershell
--allow-partial-enrollment
```

它会在某个姿态步骤未完成时仍保存已经采到的候选模板，并额外输出：

```text
partial_enrollment_reasons.json
```

这个模式用于实验，不是正式注册默认策略。

### 3. face-debug-snapshot

新增 `diagnostics_cli face-debug-snapshot`，用于本机安全排查“模型到底看见了什么”。

它不是完整引导注册，只是采样当前场景，输出：

```text
face-debug/
  annotated_frames/
  aligned_faces/
  frame_metrics.jsonl
  summary.json
```

示例：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe face-debug-snapshot `
  --camera-id opencv-index:0 `
  --scenario front `
  --start-delay-seconds 3 `
  --output-dir .\face-debug\front `
  --frames 30 `
  --frame-width 640 `
  --frame-height 480 `
  --save-aligned-face
```

支持场景标签，例如：

```text
front
yaw-left-30
yaw-right-30
pitch-up-15
pitch-down-15
backlight
low-light
glasses-reflection
usb-low-res
```

该命令只做调试，不修改模板、不修改 Service 配置、不签发授权。

### 4. face-calibrate

新增 `diagnostics_cli face-calibrate`，用于读取当前模板集，用真实摄像头采样分数分布。

示例：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe face-calibrate `
  --template .\face-enrollment\selected_templates.json `
  --camera-id opencv-index:0 `
  --scenario front `
  --output-dir .\face-calibration\front `
  --frames 100 `
  --frame-width 640 `
  --frame-height 480 `
  --threshold-min 0.40 `
  --threshold-max 0.80 `
  --threshold-step 0.05 `
  --required-consecutive 3
```

输出：

```text
face-calibration/
  front/
    calibration_frames.jsonl
    calibration_summary.json
```

报告包含：

- 无脸、多脸、模型不匹配、embedding 提取失败帧数。
- `score_min`、`score_avg`、`score_p10`、`score_p50`、`score_p90`、`score_max`。
- 每个阈值的通过率。
- 按连续帧要求计算是否可通过。
- 检测和 embedding 提取耗时。

这个命令只给阈值调优提供数据，不直接修改 Service 阈值。

## 关键文档

当前相关文档：

```text
docs/DETAILED_IMPLEMENTATION_PLAN.md
docs/CREDENTIAL_STORE_DESIGN.md
docs/PHASE3_SERVICE_VALIDATION.md
docs/PHASE4_FACE_VALIDATION.md
docs/PHASE5_CREDENTIAL_PROVIDER_VALIDATION.md
docs/PHASE5_LOGONUI_WAKE_FIX.md
docs/PHASE5_5_FACE_AUTH_CALIBRATION.md
docs/FACE_ENROLLMENT_TEMPLATE_STRATEGY.md
docs/FACE_MODULE_COMPOSITION.md
docs/FACE_WORKFLOW_MODULE_ARCHITECTURE_AND_LIVENESS.md
docs/FACE_RECOGNITION_TECH_ROUTE_RESEARCH.md
docs/MEDIAPIPE_BRIDGE_ABI.md
docs/NATIVE_DLL_RUNTIME_COLLISION_FIX.md
```

建议新接手先读：

1. `docs/FACE_MODULE_COMPOSITION.md`
2. `docs/FACE_ENROLLMENT_TEMPLATE_STRATEGY.md`
3. `docs/PHASE5_5_FACE_AUTH_CALIBRATION.md`
4. `docs/PHASE5_CREDENTIAL_PROVIDER_VALIDATION.md`
5. `docs/PHASE5_LOGONUI_WAKE_FIX.md`

## 当前验证命令

每次收尾至少跑：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p diagnostics_cli
```

最近一次已经通过：

```text
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p diagnostics_cli
```

GitNexus 当前按 CLI 使用：

```powershell
npx gitnexus status
npx gitnexus analyze
npx gitnexus analyze --force
```

不要尝试 GitNexus MCP，当前项目规则是 CLI。

## 当前限制和风险

### 1. Landmark 位姿只是临时 fallback

`LandmarkFacePoseProvider` 只基于 YuNet 五点关键点估计姿态。它能做粗判断，但不能等同于手机级 3D 姿态判断。

当前 `pose-ready-min-fit` 默认是 `0.25`，是为了配合这个粗估 provider。后续 MediaPipe 可用后应重新调高。

### 2. 眨眼依赖 MediaPipe

当前 fallback 不支持 blink，所以 guided-enroll 默认不会进入眨眼步骤。不要用五点 landmark 伪造眨眼能力。

### 3. MediaPipe bridge Rust 侧已就绪，native 构建未完成

已有：

```text
crates/face_pose_mediapipe
docs/MEDIAPIPE_BRIDGE_ABI.md
native/mediapipe_bridge/
scripts/build_mediapipe_bridge.ps1
```

阻塞点是官方 MediaPipe Windows Bazel toolchain，不是 Rust 主线。不要把这个问题带进 `win_service`。

### 4. 本机可以做人脸算法调试，Provider 仍需虚拟机

本机可以做：

- `guided-enroll`
- `face-debug-snapshot`
- `face-calibrate`
- 模型 A/B 和阈值调优

必须继续放在虚拟机做：

- Provider 注册/卸载。
- LogonUI 自动登录。
- 开机未登录状态。
- Provider 崩溃恢复。
- emergency disable provider。

### 5. 调试图片不能提交

以下目录已忽略：

```text
face-debug/
face-calibration/
face-enrollment/
face-enrollment-*/
```

这些可能包含人脸图像，不应提交。

## 推荐下一步

### Step 1：跑完整 guided-enroll

用本机摄像头运行：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment `
  --accepted-frames-per-step 6 `
  --max-wait-frames-per-step 300 `
  --max-frames-per-step 180 `
  --pose-ready-consecutive 3 `
  --pose-ready-min-fit 0.25 `
  --frame-delay-ms 60 `
  --threshold 0.75 `
  --save-debug-images
```

观察每一步是否能进入：

```text
waiting_for_pose=false
pose_confirmed=true
recording_started=true
recording_frame_count=...
```

如果某一步一直卡住，先看：

```text
pose_fit_score
pose_estimate
reject_reason
debug_frames/
aligned_faces/
```

### Step 2：跑场景校准

至少跑：

```text
front
yaw-left-30
yaw-right-30
pitch-up-15
pitch-down-15
backlight
```

每个场景都用 `face-calibrate` 输出报告。不要只根据一次正脸结果改阈值。

### Step 3：根据报告调整阈值

需要关注：

```text
score_p10
score_p50
score_p90
threshold_reports
sequence_auth_would_pass
```

如果低像素 USB 摄像头下分数整体低，优先优化注册样本和姿态覆盖，不要直接大幅降低登录阈值。

### Step 4：把新模板集接回 Service

模板集稳定后，再配置 Service 使用 `selected_templates.json`，然后在虚拟机测试 Provider 登录。

真机测试前必须确认：

- PIN/密码仍可用。
- emergency disable provider 可用。
- Provider visible tile 不被禁用。
- Service 失败只导致人脸认证失败，不影响系统登录。

### Step 5：继续推进 MediaPipe 位姿增强

目标是让注册阶段获得更准确的：

```text
yaw/pitch/roll
blink score
face landmarks
facial transformation matrix
```

但它仍然只进入注册和诊断链路，不进入默认 Service 登录热路径。

## 当前代码改动范围提示

当前工作区还有大量未提交改动，包含前面多个阶段的实现，不只是本次交接文档。

提交前建议：

```powershell
git status --short
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
npx gitnexus status
```

如果要做阶段性提交，建议提交信息按能力分组，而不是一个巨大含糊提交。例如：

```text
Add guided face enrollment state machine
Add face pose provider boundary
Add face auth diagnostics and calibration commands
Document phase 5.5 face auth calibration workflow
```
