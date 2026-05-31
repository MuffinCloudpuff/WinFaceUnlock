# Phase 4 本地摄像头和人脸识别验收

Phase 4 的主线是 Rust 后端能力，不包含 UI。

## 模块边界

- `video_provider`：本地 OpenCV 摄像头枚举、打开、取帧、关闭。
- `face_engine`：YuNet/SFace 模型加载、人脸检测、embedding 提取、模板编解码、相似度比对。
- `face_auth`：图片注册、模板识别、连续成功策略、失败冷却策略。
- `diagnostics_cli`：阶段验收入口，不保存明文密码，不绕过 Credential Store 设计。
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
```

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
6. 命令结束后摄像头被释放；没有常驻占用。

默认 SFace cosine 阈值使用 OpenCV 示例推荐的 `0.363`。如果后续切换量化模型或采集环境变化，需要重新做阈值校准。
