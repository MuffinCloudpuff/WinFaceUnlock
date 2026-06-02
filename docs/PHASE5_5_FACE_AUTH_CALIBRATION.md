# Phase 5.5 Face Auth 可观测性、校准与模型路线复评

## 背景

Phase 4 已经把本地摄像头、人脸检测、人脸识别、模板匹配和 Service `WakeAuth` 链路跑通。Phase 5 已经在虚拟机中验证 Credential Provider、LogonUI 自动唤醒、`CredentialsChanged` 和凭据提交。

现在的问题不再是“能不能跑通”，而是“识别效果是否足够稳定、可解释、可调优”。目前用户只能看到最终匹配分数，缺少以下信息：

- 检测框是否正确框住人脸。
- 模型返回的关键点/对齐点在哪里。
- 偏头、侧脸、低头、抬头到什么程度会导致检测失败或匹配分下降。
- 光照、背光、摄像头角度、摄像头分辨率对识别分数的影响。
- 当前 YuNet + SFace 是否是本项目长期最优路线，还是只是参考开源项目后的早期选择。

Phase 5.5 的目标是先把 Face Auth 做成“可看、可测、可复现、可比较”，再讨论是否换模型。

## 当前模型路线

当前实现采用：

- 检测模型：OpenCV Zoo YuNet。
- 识别模型：OpenCV Zoo SFace。
- 推理方式：OpenCV DNN / OpenCV Face APIs。
- 运行策略：CPU 优先，本机摄像头实时触发，GPU/DirectML 不作为当前主路径。

这个路线的优点：

- Rust 侧集成成本低，依赖已经接入。
- 模型文件小，CPU 跑得动，适合登录链路的轻量需求。
- OpenCV 官方模型和 API 对 Windows 桌面部署比较友好。
- 检测模型和识别模型已经在代码里拆成独立 provider，后续可替换。

这个路线的风险：

- SFace 的实际识别鲁棒性需要在本机摄像头、侧脸、背光、眼镜、低像素 USB 摄像头下重新评估。
- YuNet 只提供有限关键点，适合对齐和检测可视化，但不等于完整脸部姿态估计。
- 当前已根据正脸和左转校准结果把项目默认匹配阈值收紧到 `0.75`；后续仍需用右转、低头、抬头、背光等场景继续复核。
- 只看单帧分数容易误判，应评估多帧分布、连续成功策略和失败原因。

## 参考资料

- OpenCV Zoo 的 YuNet 文档说明它是轻量人脸检测模型，并列出 WIDER Face 评估结果、固定输入形状和量化版本说明：[OpenCV Zoo YuNet README](https://github.com/opencv/opencv_zoo/blob/main/models/face_detection_yunet/README.md)。
- OpenCV 官方 DNN face tutorial 仍然把 YuNet 和 SFace 作为 OpenCV DNN 人脸检测/识别示例路线：[OpenCV DNN face tutorial](https://docs.opencv.org/4.x/d0/dd4/tutorial_dnn_face.html)。
- MediaPipe Face Landmarker 可以输出 3D 人脸关键点、表情 blendshape 和变换矩阵，更适合做姿态/可视化/活体辅助，而不是直接替代身份识别 embedding：[MediaPipe Face Landmarker](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker)。
- RetinaFace 的论文路线强调检测同时输出五点关键点，并能辅助 ArcFace 等识别模型提升复杂场景验证效果：[RetinaFace paper](https://arxiv.org/abs/1905.00641)。
- ONNX Runtime DirectML 可在 Windows 上使用 DirectML 做 GPU 推理，但官方说明 DirectML 处于维护状态，Windows 新方向更偏 WinML；因此当前不应为了 3050 GPU 过早切换主线：[ONNX Runtime DirectML EP](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html)。

## 核心问题

### 1. 我们看不到模型到底看到了什么

当前 diagnostics 输出主要是文字和分数。下一步必须增加可视化输出：

- 原始帧。
- 检测框。
- YuNet 五点关键点。
- 人脸对齐后的 crop。
- 识别 embedding 匹配分数。
- 认证策略状态：当前帧是否通过、连续通过计数、失败原因。

输出形式优先级：

1. 先做离线 PNG/JPEG 标注图，便于保存和复盘。
2. 再做实时 preview 窗口或轻量本地预览工具。
3. 最后才考虑 UI。

### 2. 我们不知道头部姿态边界

需要建立标准测试动作：

- 正脸。
- 左转/右转约 15 度、30 度、45 度。
- 低头/抬头约 15 度、30 度。
- 轻微侧脸加眼镜反光。
- 背光、普通室内光、弱光。
- 远近距离变化。

每组采集多帧，统计：

- 检测成功率。
- 单人脸正确率。
- 匹配分数 P50/P90/P95/最低值。
- 连续 N 帧通过概率。
- 平均耗时和最慢耗时。
- 失败原因分布。

如果没有姿态估计模型，角度先由用户按动作标签手动标注；后续再接 MediaPipe Face Landmarker 或其它 head pose estimator 做自动估计。

### 3. 我们还没有模型路线决策依据

不能凭“某个开源项目也这么选”就认定 YuNet + SFace 是最优路线。Phase 5.5 要把模型选择拆成两个独立问题：

- 检测模型是否足够稳定。
- 识别模型是否足够准确。

检测模型和识别模型必须继续保持热插拔，不允许再次绑定成一个不可替换的大模块。

## 候选技术路线

| 路线 | 用途 | 优点 | 风险 | 当前建议 |
|---|---|---|---|---|
| YuNet + SFace | 当前主线，轻量本机识别 | 集成成本低、CPU 友好、OpenCV 支持好 | 侧脸/背光/低像素效果需实测 | 保留为 baseline |
| YuNet + SFace + 可视化/校准 | 当前路线增强 | 最低风险，先建立数据闭环 | 不解决模型上限问题 | Phase 5.5 第一优先级 |
| MediaPipe Face Landmarker + SFace | 姿态/关键点辅助 + 现有识别 | 可视化强，可估计姿态/眨眼/表情 | MediaPipe 不应直接当身份识别模型 | 第二优先级实验 |
| RetinaFace + ArcFace/InsightFace | 更强检测和识别路线 | 复杂姿态和识别鲁棒性潜力更高 | Rust/Windows/许可/模型包体/部署成本更高 | 数据证明 SFace 不够后再评估 |
| ONNX Runtime + DirectML/WinML | 推理后端优化 | 可利用 Windows GPU/NPU | 引入新运行时复杂度，DirectML 路线需谨慎 | 不作为 Phase 5.5 第一目标 |

## Phase 5.5 任务拆分

### 任务 1：Face Auth Debug Snapshot

新增 diagnostics 命令：

```powershell
diagnostics_cli.exe face-debug-snapshot `
  --camera-id opencv-index:0 `
  --output-dir .\face-debug `
  --frames 30 `
  --draw-detections `
  --draw-landmarks `
  --save-aligned-face
```

输出：

- 每帧标注图。
- 对齐后的人脸 crop。
- JSONL 指标文件。
- 汇总报告。

当前实现状态：已完成 CLI 第一版。

实际命令：

```powershell
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe face-debug-snapshot `
  --camera-id opencv-index:0 `
  --output-dir .\face-debug `
  --frames 30 `
  --frame-width 640 `
  --frame-height 480 `
  --save-aligned-face
```

输出目录结构：

```text
face-debug/
  annotated_frames/
    00000.jpg
    ...
  aligned_faces/
    00000.jpg
    ...
  frame_metrics.jsonl
  summary.json
```

当前每帧指标包括：

- `frame_index`
- `frame_width` / `frame_height`
- `detected_face_count`
- `detection_elapsed_ms`
- `embedding_extraction_succeeded`
- `embedding_dimensions`
- `annotated_frame_path`
- `aligned_face_path`
- `faces[]`，包含检测框、五点关键点和检测置信度

这个命令只属于 `diagnostics_cli` 本机调试链路，不进入 `win_service`、Credential Provider 或 LogonUI 登录热路径。调试图片目录已加入 `.gitignore`，避免把可识别人脸图片误提交。

### 任务 2：Face Auth Calibration

新增 diagnostics 命令：

```powershell
diagnostics_cli.exe face-calibrate `
  --camera-id opencv-index:0 `
  --scenario front `
  --frames 100 `
  --threshold-range 0.40..0.80 `
  --output-dir .\face-calibration
```

输出：

- 检测成功率。
- 匹配分数分布。
- 推荐阈值区间。
- 连续成功次数建议。
- 失败原因分布。

当前实现状态：已完成 CLI 第一版。

实际命令：

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

输出目录结构：

```text
face-calibration/
  front/
    calibration_frames.jsonl
    calibration_summary.json
```

`calibration_summary.json` 当前记录：

- 检测成功/无脸/多脸/模型不匹配/embedding 提取失败帧数。
- `score_min`、`score_avg`、`score_p10`、`score_p50`、`score_p90`、`score_max`。
- 每个阈值的通过帧数、通过比例、以及按 `required_consecutive_match_count` 计算的连续帧是否可通过。
- 检测与提取 embedding 的平均耗时和最大耗时。

注意：这个命令只负责统计“当前模板 + 当前摄像头 + 当前场景标签”的真实分数分布，不直接修改 Service 阈值。阈值变更必须基于多个场景报告再决定，避免只针对某一次正脸采样过拟合。

### 任务 3：Pose / Scenario Sweep

建立人工标签场景：

```text
front
yaw-left-15
yaw-left-30
yaw-left-45
yaw-right-15
yaw-right-30
yaw-right-45
pitch-up-15
pitch-down-15
backlight
low-light
glasses-reflection
usb-low-res
```

每个场景生成独立报告，最后汇总成模型效果矩阵。

### 任务 4：模型可替换性验收

确认检测模型和识别模型仍然完全独立：

- 换 detector 不影响 recognizer。
- 换 recognizer 不影响 detector。
- 模板文件记录 recognition model identity，避免不同 embedding 模型混用。
- CLI 可以显式传入 detector/recognizer 路径。
- 测试覆盖模型不匹配时拒绝认证。

## 决策门槛

在没有数据前，不切换模型主线。满足以下任一条件后，进入模型替换评估：

- 正脸、正常光照下 P50 分数稳定，但侧脸 30 度以内明显掉到阈值以下。
- 同一用户多次采集分数方差过大，导致阈值无法同时兼顾误拒和误接收。
- 低像素 USB 摄像头下检测框或关键点明显抖动。
- SFace 对眼镜、背光、轻微低头非常敏感。
- 单次认证耗时过高，影响锁屏体验。

候选替换评估必须输出同样格式的 calibration report，不能只看主观体验。

## 本机与虚拟机边界

本机可安全做：

- 摄像头采集测试。
- 检测框/关键点可视化。
- 模板注册与重注册。
- 阈值校准。
- 模型 A/B 测试。
- 识别耗时统计。

必须继续放在虚拟机做：

- Credential Provider 注册。
- LogonUI 自动登录。
- Service 开机未登录状态。
- Provider 崩溃恢复。
- `emergency-disable-provider` 恢复路径。

## 推荐下一步

先实现 `face-debug-snapshot`，不要直接改阈值或换模型。

理由：

1. 可视化能立刻回答“检测框和关键点到底准不准”。
2. 标注图和 JSONL 指标能作为后续模型 A/B 的统一输入。
3. 不碰 Windows 登录链路，可以在本机安全高频迭代。
4. 有数据后再讨论 SFace 是否保留，避免凭主观感觉换路线。

完成 `face-debug-snapshot` 后，再实现 `face-calibrate` 和场景 sweep。
