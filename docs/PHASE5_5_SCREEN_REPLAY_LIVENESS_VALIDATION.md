# Phase 5.5 屏幕回放攻击检测模块验收

更新时间：2026-06-02

## 目标

本阶段先实现独立的 `face_liveness` 模块。完成真人与屏幕回放采样后，MiniFASNet 已接入 Windows Service 本地摄像头认证主链路；LogonUI 和 Credential Provider 不直接加载模型。

当前能力严格命名为：

```text
screen replay attack detection
```

它用于发现明显的手机、平板或显示器屏幕回放攻击，不等同于完整活体检测，也不能承诺防住打印照片、近距离裁剪视频、虚拟摄像头注入、3D 面具或 deepfake。

## 模块边界

新增 crate：

```text
crates/face_liveness/
  src/minifasnet.rs
  src/preprocessing.rs
  src/types.rs
  src/screen_replay.rs
  src/policy.rs
  src/opencv_debug.rs
```

职责：

- `preprocessing.rs`：封装屏幕检测前置图像处理，流程为灰度图、`THRESH_BINARY_INV`、`inRange(0, 50)`，输出用于找外层轮廓的二值 mask。
- `types.rs`：定义结构化的活体判定、证据和矩形区域。
- `screen_replay.rs`：复用 `preprocessing.rs` 输出的 mask，使用 `RETR_EXTERNAL` 查找最外层轮廓，再通过 `approxPolyDP`、`boundingRect`、面积、宽高比和人脸重叠比例判断手机屏幕候选。该路径现仅保留为诊断信息，不参与最终拒绝。
- `minifasnet.rs`：加载 MiniFASNet-V2 ONNX 模型，对 80x80 BGR 人脸 crop 输出 `2D fake / real / 3D fake` 三分类概率。该模型是当前活体主线，spoof 分数达到阈值时直接输出 `SpoofRejected`。
- `policy.rs`：把 provider 输出转换成 workflow 决策。
- `opencv_debug.rs`：只负责调试图绘制，不参与判定逻辑。

没有把活体代码写进：

```text
face_engine
face_auth::FaceAuthenticator
win_service
windows_provider
```

Windows Service 本地摄像头 workflow 已在“人脸检测”和“embedding 提取”之间插入 `MiniFasNetLivenessProvider` 调用。

## 第一版判定语义

```text
MiniFASNet 输出 fake 分数超过阈值
-> SpoofRejected

检测到屏幕矩形，并且人脸大部分位于矩形内部
-> 仅记录几何诊断信息，不参与最终拒绝

没有检测到可靠证据
-> Inconclusive

检测到背景屏幕，但人脸不在屏幕内部
-> Inconclusive
```

`Inconclusive` 不等于失败。第一版启发式算法只负责发现强证据，不应因为证据不足误伤正常真人登录。

## 独立调试命令

预览同源预处理效果：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe threshold-preview `
  --camera-id opencv-index:0 `
  --frame-width 640 `
  --frame-height 480 `
  --method binary-inv-mask `
  --binary-threshold 150 `
  --binary-mask-upper-threshold 50
```

窗口左侧为原图，右侧为 `face_liveness` 模块实际使用的二值 mask。这个命令用于调参，不进入登录主链路。

先编译：

```powershell
cd D:\study\workspace\Rust_workspace\WinFaceUnlock
cargo build -p diagnostics_cli
```

正常真人场景：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe liveness-screen-debug `
  --camera-id opencv-index:0 `
  --output-dir .\liveness-debug\real-person `
  --frames 100 `
  --frame-width 640 `
  --frame-height 480 `
  --save-debug-images
```

如果 `models/minifasnet_v2.onnx` 存在，该命令会同时启用 MiniFASNet 静默活体诊断。也可以显式指定模型路径：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe liveness-screen-debug `
  --camera-id opencv-index:0 `
  --output-dir .\liveness-debug\phone-replay-minifasnet `
  --frames 100 `
  --frame-width 640 `
  --frame-height 480 `
  --minifasnet-model .\models\minifasnet_v2.onnx `
  --save-minifasnet-crops `
  --save-debug-images
```

`--save-minifasnet-crops` 会额外输出 MiniFASNet 实际输入图：

```text
minifasnet_crops/
  source/              # resize 前的 BGR crop，能看裁剪范围
  model_input_80x80/   # 送入模型的 80x80 BGR 图
```

屏幕矩形几何检测默认不加载、不执行。如需离线复查旧算法，可显式加：

```powershell
--enable-screen-geometry-diagnostics
```

该开关只会增加诊断字段和标注图，不会影响最终拒绝结论。需要单独观察几何算法时，可同时加 `--disable-minifasnet`。

MiniFASNet 默认参与拒绝。如需只采集模型分数、不让模型影响窗口结论，可加：

```powershell
--minifasnet-diagnostic-only
```

注意：本地 ONNX 模型输入必须使用 80x80 BGR 原始 `0-255` 像素值。错误地除以 `255` 会导致真人稳定误判为 replay attack。

手机播放人脸视频场景：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe liveness-screen-debug `
  --camera-id opencv-index:0 `
  --output-dir .\liveness-debug\phone-replay `
  --frames 100 `
  --frame-width 640 `
  --frame-height 480 `
  --save-debug-images
```

背景屏幕但真人在屏幕外的场景：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe liveness-screen-debug `
  --camera-id opencv-index:0 `
  --output-dir .\liveness-debug\background-screen `
  --frames 100 `
  --frame-width 640 `
  --frame-height 480 `
  --save-debug-images
```

## 输出文件

每次运行会输出：

```text
liveness_metrics.jsonl
liveness_summary.json
annotated_frames/
```

关键字段：

```text
detected_face_count
screen_like_rectangle_count
screen_rectangle_detected_frame_count
face_inside_screen_candidate_frame_count
best_screen_observation
liveness_decision
liveness_score
minifasnet_liveness_result
evidence
elapsed_ms
```

计数关系：

```text
single_face_frame_count + multiple_face_frame_count + no_face_frame_count = captured_frame_count
```

这三项描述“人脸检测结果”，互斥相加。

```text
screen_rectangle_detected_frame_count
face_inside_screen_candidate_frame_count
spoof_rejected_frame_count
inconclusive_frame_count
```

这几项描述“屏幕回放检测结果”，和人脸检测计数不是同一组分类，不能直接与人脸帧数相加。

`screen_rectangle_detected_frame_count` 只表示画面里检测到了矩形区域，可能是窗户、背景亮面、显示器或手机屏幕。几何诊断默认关闭，因此该字段默认为 `0`。只有显式启用 `--enable-screen-geometry-diagnostics` 后才会计算。

`face_inside_screen_candidate_frame_count` 表示人脸落在矩形区域内。该字段只属于可选几何诊断，不影响 MiniFASNet 主线拒绝。

`spoof_rejected_frame_count` 是 MiniFASNet 主线明确拒绝的帧数。屏幕矩形几何结果只保留为诊断信息，不再影响该计数。

`auth_window_spoof_rejected` 表示本次采样窗口的最终活体拦截结论。只要窗口内任一 MiniFASNet 主线结果出现 `SpoofRejected`，该字段就为 `true`，含义是这一次认证窗口应整体拒绝。

`minifasnet_model_spoof_score_frame_count` 表示 MiniFASNet 分数达到假体阈值的帧数，只用于诊断模型倾向。

`minifasnet_spoof_rejected_frame_count` 表示 MiniFASNet 实际参与拒绝的帧数。默认情况下 MiniFASNet 会输出 `SpoofRejected` 并影响 `auth_window_spoof_rejected`。只有显式启用 `--minifasnet-diagnostic-only` 时才只记录分数。

`inconclusive_frame_count` 不是“判定为真人”，只是“MiniFASNet 证据不足”。

注意：`liveness-screen-debug` 的 `auth_window_spoof_rejected` 是离线诊断汇总字段。Windows Service 正式解锁链路使用动态窗口比例：单帧 spoof 只累计计数，不立即拒绝；形成连续真人匹配的解锁候选时，如果截至当前帧的 MiniFASNet spoof 比例大于 `0.40`，才拒绝本次认证窗口。否则立即解锁，不强制等待满 30 帧。

显式启用几何诊断后，标注图中：

- 绿色框：YuNet 检出的人脸。
- 红色框：屏幕候选矩形，并且人脸位于矩形内部。
- 橙色框：屏幕候选矩形，但人脸不在矩形内部。

## 可调阈值

CLI 支持以下参数：

```text
--binary-threshold
--binary-mask-upper-threshold
--min-screen-area-ratio
--max-screen-area-ratio
--min-rectangularity-score
--min-brightness-contrast-score
--min-face-inside-screen-ratio
--min-screen-aspect-ratio
--max-screen-aspect-ratio
--minifasnet-model
--minifasnet-min-live-score
--minifasnet-min-spoof-score
--minifasnet-crop-scale
--save-minifasnet-crops
--minifasnet-diagnostic-only
--disable-minifasnet
--enable-screen-geometry-diagnostics
```

默认值只是本机采样前的初始值。不要在没有真人场景和攻击场景数据前把该模块默认接入 Windows 登录拦截。

## 独立模块验收标准

1. 正常真人画面下，`spoof_rejected_frame_count` 应接近 0。
2. 手机屏幕播放人脸视频时，`spoof_rejected_frame_count` 应显著升高。
3. 背景存在手机或显示器，但真人不在屏幕区域内时，不应频繁拒绝。
4. `annotated_frames` 可以解释每次拒绝命中了哪个矩形。
5. `cargo test -p face_liveness` 通过。
6. `cargo test -p diagnostics_cli` 通过。

## 主线接入后的验证项

完成上述三类场景采样后，继续验证：

1. 是否调整阈值。
2. 是否调整 MiniFASNet 多帧窗口策略。
3. VM 和真机 Service 登录链路是否稳定写入 `liveness_score`。

Windows PIN 和密码保底路径不受影响。

## MiniFASNet 模型来源和许可

当前默认模型：

```text
models/minifasnet_v2.onnx
```

来源：

```text
ONNX: https://huggingface.co/garciafido/minifasnet-v2-anti-spoofing-onnx
Upstream: https://github.com/minivision-ai/Silent-Face-Anti-Spoofing
License: Apache-2.0
SHA-256: d7b3cd9ba8a7ceb13baa8c4720902e27ca3112eff52f926c08804af6b6eecc7b
Input: 80x80 BGR crop, raw 0-255 pixel values, NCHW
Classes: 2D fake, real, 3D fake
```

该模型不提交进 Git。发布产品时需要保留 Apache-2.0 许可证和上游 attribution。Apache-2.0 允许商用、修改和再分发，但不提供效果担保；是否能覆盖本项目摄像头、光线和屏幕回放攻击场景，必须用本机采样结果验证。

## 当前屏幕回放召回问题记录与修正

2026-06-01 的后续调试发现：当前启发式检测能拒绝部分屏幕回放帧，但还不能稳定拒绝全部屏幕回放帧。一组 100 帧样本中出现过：

```text
captured_frame_count = 100
single_face_frame_count = 73
no_face_frame_count = 27
spoof_rejected_frame_count = 26
inconclusive_frame_count = 74
```

对逐帧明细复盘后，漏拒帧主要分为：

```text
no_face = 27
face_but_no_screen_rectangle = 12
face_and_rectangle_but_face_not_fully_inside = 35
spoof_rejected_face_fully_inside_rectangle = 26
```

其中 `face_and_rectangle_but_face_not_fully_inside` 不是“完全没有命中屏幕”，多帧的人脸与屏幕候选区域重叠率已经达到 0.96/0.99。根因是当前的严格 `fully_contains(face_box)` 判定把 YuNet 的外扩人脸检测框当成真实人脸边界，而 OpenCV 轮廓框又更接近屏幕里的亮内容区域，不一定覆盖手机/屏幕物理边框。

因此不应继续只围绕单个亮度阈值微调，而应调整判定语义。当前已经落地的规则是：

```text
强证据：
  screen_rectangle_fully_contains_face_box

补充证据：
  face_screen_overlap_ratio >= min_face_inside_screen_ratio
  当前默认 min_face_inside_screen_ratio = 0.95
```

这样仍然表达“人脸在屏幕里”，但不把检测框外扩误差当成真人证据。

同时，登录接入不能按单帧 `Inconclusive` 直接放行。已经新增短窗口/会话级策略：

```text
窗口内任一帧 SpoofRejected
-> 拒绝本次认证

窗口内没有 SpoofRejected
-> 允许后续人脸识别流程继续判断
```

也就是说，`spoof_rejected_frame_count = 26` 对调试来说说明召回率还要提高；但对真实登录流程来说，只要这些证据出现在认证窗口内，就不应该让同一次认证继续按后续帧放行。

## 本机真人基线记录

2026-06-01 已在本机执行一组短采样：

```text
camera_id = opencv-index:0
frame_width = 640
frame_height = 480
captured_frame_count = 30
single_face_frame_count = 15
no_face_frame_count = 15
screen_candidate_frame_count = 0
spoof_rejected_frame_count = 0
average_elapsed_ms = 54.33
```

当时画面存在侧脸、弱光和窗边高亮区域，第一版默认阈值没有产生屏幕回放误拒。该记录只能说明真人基线初步正常，不能替代手机回放攻击采样。
