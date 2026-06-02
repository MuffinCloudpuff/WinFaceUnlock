# MediaPipe 位姿 Bridge ABI

## 背景

MediaPipe Face Landmarker 官方主线是 C++/Python/Web/Android/iOS，没有可直接用于本项目的 Rust/Windows 原生绑定。为避免把 Bazel、C++ MediaPipe 和大体积依赖拉进 Windows Service，Rust 侧通过 `face_pose_mediapipe` 动态加载独立 DLL bridge。

这个 bridge 只服务注册和诊断链路，不进入 Credential Provider 或 `win_service` 默认登录链路。

## 文件约定

```text
native/winfaceunlock_mediapipe_bridge.dll
models/face_landmarker.task
```

Rust CLI 参数：

```powershell
cargo build -p diagnostics_cli --features mediapipe-pose

--pose-provider mediapipe
--mediapipe-bridge .\native\winfaceunlock_mediapipe_bridge.dll
--mediapipe-model .\models\face_landmarker.task
```

## 导出函数

DLL 需要导出三个 C ABI 函数：

```cpp
extern "C" void* winfaceunlock_mediapipe_pose_create(
    const char* model_path,
    WinFaceUnlockMediaPipeOptions options);

extern "C" void winfaceunlock_mediapipe_pose_destroy(void* provider);

extern "C" int winfaceunlock_mediapipe_pose_estimate(
    void* provider,
    const WinFaceUnlockMediaPipeFrameRequest* request,
    WinFaceUnlockMediaPipePoseResult* result);
```

返回约定：

- `create` 返回 `nullptr` 表示创建失败。
- `estimate` 返回 `0` 表示成功，非 0 表示推理失败。
- bridge 内部不得保存 Rust 传入的帧数据指针；如需异步处理必须自行复制。

## 数据结构

Rust 侧当前结构布局：

```cpp
struct WinFaceUnlockMediaPipeOptions {
  uint32_t running_mode; // 0=image, 1=video
  uint8_t output_face_blendshapes;
  uint8_t output_facial_transformation_matrixes;
  uint8_t reserved[6];
};

struct WinFaceUnlockMediaPipeFrameRequest {
  uint32_t width;
  uint32_t height;
  uint32_t pixel_format; // 0=BGR8, 1=RGB8, 2=GRAY8
  uint32_t reserved;
  const uint8_t* data;
  size_t data_len;
  float face_box_x;
  float face_box_y;
  float face_box_width;
  float face_box_height;
};

struct WinFaceUnlockMediaPipePoseResult {
  float yaw_deg;
  float pitch_deg;
  float roll_deg;
  float left_eye_blink_score;
  float right_eye_blink_score;
};
```

## MediaPipe 输出映射

建议 bridge 使用 Face Landmarker 的：

- `facial_transformation_matrixes`：换算 yaw/pitch/roll。
- `face_blendshapes`：读取 `eyeBlinkLeft` 和 `eyeBlinkRight`，映射到 `0.0..1.0`。
- `face_landmarks`：可作为后续可视化和质量诊断输出，但不要经 ABI 返回大数组，避免注册热循环复制过重。

## 错误和日志

bridge 日志只能记录：

- 模型加载是否成功。
- 推理耗时。
- 是否检测到 face landmarker 输出。
- 错误码。

不得记录：

- Windows 密码或凭据。
- 完整 embedding。
- 不必要的人脸原图。

## 验收

Rust adapter 编译验证：

```powershell
cargo test -p face_pose_mediapipe
```

Service 不带 MediaPipe 验证：

```powershell
cargo test -p face_auth --no-default-features
cargo check -p win_service
```

Bridge 源码语法验证：

```powershell
$vsPath = "D:\study\Microsoft Visual Studio\2022\Community"
$devCmd = Join-Path $vsPath "Common7\Tools\VsDevCmd.bat"
cmd /c "`"$devCmd`" -arch=x64 -host_arch=x64 >nul && cl /nologo /std:c++17 /EHsc /DWINFACEUNLOCK_MEDIAPIPE_BRIDGE_EXPORTS /I native\mediapipe_bridge\include /I .external\mediapipe /c native\mediapipe_bridge\src\winfaceunlock_mediapipe_bridge.cpp /Fo:target\winfaceunlock_mediapipe_bridge_syntax.obj"
```

MediaPipe 实机注册验证在 DLL 和 `.task` 文件准备好后执行：

```powershell
cargo build -p diagnostics_cli --features mediapipe-pose

.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment-mediapipe `
  --pose-provider mediapipe `
  --mediapipe-bridge .\native\winfaceunlock_mediapipe_bridge.dll `
  --mediapipe-model .\models\face_landmarker.task `
  --save-debug-images
```

## 2026-06-01 Windows Bazel 构建记录

本机已确认：

- MSVC 可通过 `D:\study\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat` 加载。
- Bazelisk 1.29.0 已安装。
- Python 3.11 可用。
- `face_landmarker.task` 已下载到 `models/face_landmarker.task`。

尝试构建官方 C API target：

```powershell
bazelisk build -c opt `
  --repo_env=HERMETIC_PYTHON_VERSION=3.11 `
  --define MEDIAPIPE_DISABLE_GPU=1 `
  //mediapipe/tasks/c/vision/face_landmarker:face_landmarker_c_lib
```

结果：

- 使用 MediaPipe 仓库 `.bazelversion` 指定的 Bazel 7.4.1 时，分析阶段失败在 `@rules_java//tools/jdk`。
- 强制 Bazel 8.4.0 / 9.1.0 时，失败在 rules_java 的 Java provider 兼容性。

结论：

1. 当前 bridge ABI 和源码已经就绪。
2. 当前阻塞点是官方 MediaPipe Windows Bazel toolchain，不是 Rust 主线或 bridge ABI。
3. 不应把这个构建问题带进 `win_service` 或 Credential Provider。
