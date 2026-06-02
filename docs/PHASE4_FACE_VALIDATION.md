# Phase 4 本地摄像头和人脸识别验收

Phase 4 的主线是 Rust 后端能力，不包含 UI。

## 模块边界

- `video_provider`：本地 OpenCV 摄像头枚举、打开、取帧、关闭。
- `face_engine`：检测模型 provider、识别模型 provider、组合 pipeline、模板编解码和相似度比对。当前默认实现分别是 YuNet 和 SFace。
- `face_auth`：图片注册、模板识别、连续成功策略、失败冷却策略。
- `diagnostics_cli`：阶段验收入口，不保存明文密码，不绕过 Credential Store 设计。
- `win_service`：`WakeAuth` 主链路的真实摄像头识别承载方；Credential Provider 后续只调用这条链路。
- `credential_store`：模板持久化只接收不透明 bytes，不理解模型内部格式。

## 模型文件

默认路径：

```powershell
models\face_detection_yunet_2023mar.onnx
models\face_recognition_sface_2021dec.onnx
```

模型来源使用 OpenCV Zoo：

- YuNet：`https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet`
- SFace：`https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface`

也可以通过命令参数覆盖：

```powershell
--yunet-model <path>
--sface-model <path>
```

## 模型热插拔边界

`face_engine` 将检测模型和识别模型拆成两个独立 contract：

```text
FaceDetectionModelProvider
  load_detection_model()
  detect(frame) -> face boxes / landmarks

FaceRecognitionModelProvider
  load_recognition_model()
  recognition_model() -> model family / version
  extract(frame, face) -> embedding
  compare(enrolled, candidate) -> score

FaceModelPipeline
  swap_detector(next_detector)
  swap_recognizer(next_recognizer)
```

替换模型时先加载新 provider，加载成功后才卸载旧 provider；新模型加载失败时继续保留旧模型。

- 单独替换检测模型：不要求重新生成 SFace embedding，但需要重新验证检测阈值、误检率和漏检率。
- 单独替换识别模型：必须重新注册模板并重新校准匹配阈值。模板包含识别模型族和版本；认证器只会使用与当前识别模型兼容的模板。
- 新增其他后端：实现对应 provider contract 后接入组合 pipeline，不需要修改 `face_auth` 的认证策略。

## 验收命令

```powershell
cargo build -p diagnostics_cli
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe list-cameras
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe test-camera
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe test-face --image .\samples\face-1.jpg
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe enroll-face --image .\samples\face-1.jpg --template-out .\target\phase4-face-template.json --user-id dev-user
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe enroll-camera --template-out .\target\phase4-face-template.json --camera-id opencv-index:0
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe verify-face --image .\samples\face-2.jpg --template .\target\phase4-face-template.json
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe camera-auth --template .\target\phase4-face-template.json --camera-id opencv-index:0
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe calibrate-threshold --template .\target\phase4-face-template.json --camera-id opencv-index:0 --samples 20 --max-frames 120
```

## Service 主链路验收

`camera-auth` 只验证 diagnostics 进程内的人脸识别。Phase 4 还必须验证 Service 收到 `WakeAuth` 后能自己拉起摄像头识别。

先在当前 PowerShell 会话配置 Service 的真实摄像头认证模式：

```powershell
$env:WINFACEUNLOCK_AUTH_MODE = "local-camera"
$env:WINFACEUNLOCK_FACE_TEMPLATE_PATH = "$PWD\target\phase4-face-template.json"
$env:WINFACEUNLOCK_CAMERA_ID = "opencv-index:0"
$env:WINFACEUNLOCK_YUNET_MODEL_PATH = "$PWD\models\face_detection_yunet_2023mar.onnx"
$env:WINFACEUNLOCK_SFACE_MODEL_PATH = "$PWD\models\face_recognition_sface_2021dec.onnx"
$env:WINFACEUNLOCK_MAX_AUTH_FRAMES = "30"
$env:WINFACEUNLOCK_REQUIRED_CONSECUTIVE = "2"
```

OpenCV SFace 示例常用 cosine 阈值是 `0.363`，但本项目 Service 主链路当前默认阈值已经根据 Phase 5.5 初步校准提高到 `0.75`。如果当前摄像头角度、光照或旧模板导致 Service 链路验收无法通过，可以临时设置较低阈值来验证 IPC 和 Service 编排链路，例如：

```powershell
$env:WINFACEUNLOCK_REQUIRED_CONSECUTIVE = "1"
$env:WINFACEUNLOCK_MATCH_THRESHOLD = "0.10"
```

这个低阈值只能用于链路排障，不能作为正式登录策略。正式策略需要重新采集正脸模板或做阈值校准。

阈值校准使用：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe calibrate-threshold --template .\target\phase4-face-template.json --camera-id opencv-index:0 --samples 20 --max-frames 120
```

输出会包含 `score_min`、`score_avg`、`score_max`、`score_p10`、`score_p50`、`score_p90`，以及 `0.55`、`0.60`、`0.75`、`0.85` 四个候选阈值的通过帧数。

启动一次性 Service IPC host，并让它至少处理 `WakeAuth` 和 `FetchCredential` 两个请求：

```powershell
.\target\x86_64-pc-windows-msvc\debug\win_service.exe --pipe-once --pipe-requests 2
```

在另一个 PowerShell 会话触发 Service 真实摄像头认证和凭据引用兑换：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe service-camera-auth --session-id phase4-camera-service-test
```

也可以分步验收。先触发真实摄像头 `WakeAuth`：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe wake-auth --source local-camera --session-id phase4-camera-service-test
```

成功后继续使用输出的 `grant_id` 和 `nonce` 取凭据引用：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe fetch-credential --session-id phase4-camera-service-test --grant-id <grant_id> --nonce <nonce>
```

未设置 `WINFACEUNLOCK_AUTH_MODE=local-camera` 时，`wake-auth` 默认仍使用 `manual-test` 模拟链路，保证 Phase 3 验收不受影响；`service-camera-auth` 始终请求 `local-camera`。

## 摄像头选择和分辨率

摄像头来源通过 `--camera-id` 选择，当前 OpenCV 本地摄像头 ID 格式是：

```powershell
opencv-index:0
opencv-index:1
```

先用 `list-cameras` 查看可用摄像头，再把选中的 ID 传给 `test-camera`、`enroll-camera` 或 `camera-auth`。

分辨率可以通过以下参数请求：

```powershell
--frame-width 640 --frame-height 480
```

摄像头驱动可能不会完全接受请求分辨率，所以实际识别分辨率以 `test-camera` 输出的 `frame: width=... height=...` 为准。当前本机默认读到的是 `640x480`，`face_engine` 会把 YuNet 的输入尺寸设置为实际帧尺寸，不在认证链路里额外固定缩放。SFace 会在检测框基础上执行 `align_crop`，再提取 embedding。

## 通过标准

1. `test-camera` 输出 `camera_count`，并读取到非空帧。
2. `test-face` 输出 `detected_face_count`，单人脸图片输出 `embedding_dimensions`。
3. `enroll-face` 生成模板文件。
4. `verify-face` 输出 `auth_match_passed: true`。
5. `camera-auth` 输出 `camera_auth_passed: true`。
6. `diagnostics_cli service-camera-auth` 返回 `AuthSucceeded` 和 `CredentialReady`。
7. 分步执行时，`fetch-credential` 能使用真实识别产生的 grant 取得 `CredentialReady`。
8. `calibrate-threshold` 能输出有效分数分布，用于后续阈值收紧。
9. 命令结束后摄像头被释放；没有常驻占用。

OpenCV SFace 官方示例阈值是 `0.363`，本项目 Service 主链路当前默认使用 `0.75` 作为初步收紧值。如果后续切换量化模型或采集环境变化，需要重新做阈值校准。
