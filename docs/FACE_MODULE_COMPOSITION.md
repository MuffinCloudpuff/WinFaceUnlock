# 人脸模块按需编排边界

## 目标

人脸能力不做成一条固定大链路，而是拆成可组合模块。不同使用场景只加载自己需要的模块，避免注册期的质量检测、位姿检测、调试导出和后续 MediaPipe 依赖进入 Windows 登录热路径。

## 模块职责

```text
video_provider
  摄像头枚举、打开、读帧。

face_engine
  人脸检测 provider、识别 provider、embedding 提取、模板比对。
  检测模型和识别模型是两个独立 contract，可以分别替换。

face_pose
  位姿估计统一接口。
  当前 fallback 是 LandmarkFacePoseProvider，后续 MediaPipe adapter 也只实现这个接口。

face_pose_mediapipe
  MediaPipe Face Landmarker adapter。
  通过动态 DLL bridge 加载，不被 win_service 默认链接。

face_auth(auth core)
  登录认证核心：单帧检测、embedding 提取、多模板匹配、连续帧策略、冷却策略。
  不依赖位姿检测、质量评分、debug 图片导出。

face_auth(enrollment feature)
  注册链路：引导步骤、质量评分、位姿匹配、模板筛选、注册报告。

diagnostics_cli
  调试入口：guided-enroll、face-auth-debug、enrollment-report、图片导出。
  face-debug-snapshot 这类本机调试命令也放在这里，不进入 Service。

win_service
  Windows 登录热路径承载方。
  只依赖 face_auth 的认证核心，不启用 enrollment feature。
```

## 当前 Cargo 边界

`face_auth` 默认启用 `enrollment` feature，方便 diagnostics CLI 和本机调试：

```toml
[features]
default = ["enrollment"]
enrollment = ["dep:face_pose"]
```

`win_service` 明确关闭默认 feature：

```toml
face_auth = { path = "../face_auth", default-features = false }
```

这表示 Service 登录链路只编译认证核心。注册时需要的 `face_pose`、质量评分和 guided enrollment 不会被 `win_service` 直接带入。

## 使用场景编排

### Windows 登录识别

```text
win_service
  -> video_provider
  -> face_engine detector
  -> face_engine recognizer
  -> face_auth::FaceAuthenticator
  -> credential grant
```

登录链路只做身份认证必需步骤：

1. 摄像头读帧。
2. 检测单张人脸。
3. 对齐并提取 embedding。
4. 和已启用模板集取最高匹配分。
5. 连续帧策略通过后签发授权。

登录链路不做：

- 引导动作判断。
- 注册样本质量打分。
- MediaPipe 位姿增强。
- debug 图片保存。
- 模板筛选。

### 引导注册

```text
diagnostics_cli guided-enroll
  -> video_provider
  -> face_engine detector
  -> face_engine recognizer
  -> face_pose provider
  -> face_auth enrollment feature
  -> selected_templates.json / enrollment_report.json
```

注册链路可以加载更多模块，因为它运行在桌面诊断环境，不在 LogonUI 热路径内：

1. 按动作步骤进入 `waiting_for_pose`。
2. 检测人脸并调用位姿 provider 判断正脸、左右转头、低头、抬头或眨眼。
3. 连续多帧姿态和质量达标后，进入 `recording_started`。
4. 录制阶段才保存候选样本并提取 embedding。
5. 质量评分和拒绝原因结构化记录。
6. 每个姿态组选 top-k 模板。
7. 输出注册报告和可选 debug 图片。

`waiting_for_pose` 阶段只做姿态门控，不保存模板，避免用户还没摆好动作时把过渡帧写进注册集。`recording` 阶段如果姿态偏离，也不会计入该步骤样本。

### 人脸算法调试

```text
diagnostics_cli face-debug-snapshot / face-calibrate / face-auth-debug / enrollment-report
  -> 可选读取模板集、摄像头、debug 输出
```

调试链路可以保存图片、打印分数、比较不同 provider。这些行为必须保持显式命令触发，不能默认进入 Service 或 Provider。

`face-debug-snapshot` 当前职责是采样摄像头帧，输出检测框、五点关键点、对齐 crop、逐帧 JSONL 指标和汇总报告。它只依赖 `video_provider`、`face_engine` 和 `diagnostics_cli` 文件输出模块，不依赖 `face_auth(enrollment)` 或 `face_pose`，因此可单独用于检测/对齐问题排查。

`face-calibrate` 当前职责是读取注册模板集，对真实摄像头帧统计匹配分数分布、阈值通过率和连续帧通过情况。它复用 `face_auth::RecognitionTemplates` 的模板契约，但不修改 Service 配置、不签发授权，也不进入 LogonUI 热路径。

## 后续 MediaPipe 接入原则

MediaPipe 只作为独立位姿 provider 接入：

```text
face_pose_mediapipe
  -> impl FacePoseProvider
```

当前 Rust 侧已经提供 `face_pose_mediapipe` crate。它不把 MediaPipe C++ 直接编进 Rust workspace，而是在执行时加载独立 bridge：

```text
native/winfaceunlock_mediapipe_bridge.dll
models/face_landmarker.task
```

这样做的原因是：

1. 官方 MediaPipe Face Landmarker 没有 Rust/Windows 原生 crate。
2. C++/Bazel 构建复杂度应隔离在 adapter/bridge 边界内。
3. `win_service` 登录热路径不能因为注册期位姿增强而加载 MediaPipe。

接入时应满足：

1. 不修改 `FaceAuthenticator`。
2. 不修改 Windows Credential Provider。
3. 不要求 `win_service` 启用 enrollment feature。
4. diagnostics CLI 通过 `--pose-provider landmark|mediapipe` 选择后端。
5. 同一套 guided-enroll 流程可以对比两个 provider 的完成率、误拒原因和耗时。

### CLI 选择

默认使用轻量 fallback：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment `
  --pose-provider landmark
```

启用 MediaPipe 位姿增强：

```powershell
cargo build -p diagnostics_cli --features mediapipe-pose

.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment-mediapipe `
  --pose-provider mediapipe `
  --mediapipe-bridge .\native\winfaceunlock_mediapipe_bridge.dll `
  --mediapipe-model .\models\face_landmarker.task
```

`landmark` provider 只声明 head pose 能力，不声明 blink 能力，所以注册流程会跳过眨眼步骤。`mediapipe` provider 声明 head pose + blink 能力，后续可以参与眨眼动作判断。

`mediapipe-pose` feature 必须显式开启。默认 diagnostics CLI 不链接 MediaPipe native bridge，`win_service` 更不会加载该 DLL。

## 验证命令

认证核心单独编译：

```powershell
cargo test -p face_auth --no-default-features
cargo check -p win_service
```

完整注册/诊断链路编译：

```powershell
cargo test -p face_auth
cargo check -p diagnostics_cli
```

全仓库收尾：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```
