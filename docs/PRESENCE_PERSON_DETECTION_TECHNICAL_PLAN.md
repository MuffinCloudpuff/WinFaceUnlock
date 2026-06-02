# Presence Person Detection 技术方案

## 背景

当前 Presence Lock 已经接入 `WinFaceUnlockService`，可以在用户登录桌面后启动后台 monitor，并在判断离座后调用锁屏。当前实现的主要判据仍是人脸链路：

```text
检测到本人脸 -> 认为用户在场
连续检测不到人脸 -> 认为可能离座
连续检测到非本人脸 -> 认为陌生人靠近
```

这个方案能跑通端到端闭环，但不适合作为长期默认策略。普通摄像头下，人脸检测很容易受到转头、低头、侧脸、遮挡、光线变化、眼镜反光和摄像头角度影响。用户仍在屏幕前，只是脸不可见时，系统不应该误判为离座。

下一步应把 Presence Lock 的主判据从 `face present` 调整为 `person bbox present and tracked`。人脸识别只作为增强信号，用于确认当前用户本人或触发陌生人审计，不再作为离座锁屏的唯一依据。

## 当前实现评价

### 已经做对的部分

1. 登录解锁链路和离座锁屏链路已经解耦。
2. Presence Monitor 只在已登录桌面 session 中运行，不参与未登录阶段的 Credential Provider 主链路。
3. Credential Provider 已调整为等待用户输入后再启动一组三次识别，符合锁屏页唤醒语义。
4. `SessionLocker` 已隔离平台锁屏实现。
5. 认证 runtime 已从 service 常驻状态移出，登录认证结束后会释放重模型资源。

### 当前主要问题

1. Face-only presence 容易误锁。用户转头或低头时可能被当成 `NoFaceDetected`。
2. 当前可配置项不足。代码中实际接入的 presence 配置主要只有：
   - `PresenceLockEnabled`
   - `PresenceOwnerMatchThreshold`
3. 文档规划了间隔、次数、审计等开关，但还未完整接入 registry 和 installer。
4. 现有运行时内存会随 monitor 状态变化。VM 实测空闲 service 约 20-30 MB Working Set；presence/camera/model 运行时可升到约 70-130 MB。

## 目标

1. 用人体框判断用户是否仍在屏幕前，降低转头、低头导致的误锁。
2. 检测 person bbox 向画面边界移动、面积缩小或消失的离开过程。
3. 保持本地处理，不保存视频帧，不上传画面。
4. 默认资源占用可接受：常驻低 FPS 检测，而不是高 FPS 实时视频分析。
5. 保留完整开关，允许用户在低内存、低 CPU、低误锁和高安全之间取舍。

## 非目标

1. 第一版不做完整人体姿态理解。
2. 第一版不做动作识别、坐姿识别或视线识别。
3. 第一版不依赖小车、24G 毫米波雷达或外部传感器。
4. 第一版不使用 Python sidecar 或 Python 常驻服务。
5. 第一版不保存连续摄像头画面。

## 为什么不用 Face-Only

Face-only 的错误来源不是简单调阈值能解决的。人脸检测需要脸部可见，而 Presence Lock 只需要知道人是否还在电脑前。

典型误判场景：

```text
用户转头看旁边屏幕
用户低头看手机
用户侧脸或背光
用户戴口罩、帽子或被麦克风遮挡
用户离摄像头太近或太远
```

这些场景下，人仍然在座位上，但 face detector 可能失败。把 `NoFaceDetected` 直接等价为离座，会导致误锁。

因此新的主判据应变为：

```text
person bbox 仍在屏幕前预期区域 -> 用户仍在场
person bbox 连续向画面边界移动或消失 -> 用户可能离开
face owner match -> 强化本人在场判断
unknown face -> 可选审计和安全加速
```

## 用户期望的离开定义

这里的“离开画面”不是简单的“人体有没有移动”。静坐用户可能长期不动，但仍然在场。

本项目中的离开应定义为：

```text
检测到 person bbox
-> bbox 中心点连续向左、右、上或下边界移动
-> 或 bbox 面积持续变小
-> 或 bbox 与座位 ROI 的重叠比例持续下降
-> 最终 bbox 消失或接近画面边界
-> 触发离开锁屏
```

需要跟踪的不是像素级运动，而是 person bbox 的时序变化。

## 技术路线选择

### 推荐第一版：OpenCV DNN + MobileNet-SSD/SSDLite

第一版推荐使用 OpenCV DNN 跑轻量 person detector，例如 MobileNet-SSD 或 SSDLite，只读取 COCO `person` 类。

选择理由：

1. 项目已经依赖 OpenCV，新增部署面最小。
2. MobileNet-SSD/SSDLite 输入尺寸通常为 300x300 或 320x320，计算量低于 640x640 YOLO。
3. 第一版只需要 person bbox，不需要多类别复杂检测。
4. 后处理比 YOLO 简单。
5. 可以直接在 Rust 主线中封装，不引入 Python sidecar。

参考：

- OpenCV SSD MobileNet DNN 示例：`https://docs.opencv.org/4.x/d4/d2f/tf_det_tutorial_dnn_conversion.html`
- OpenCV DNN efficiency：`https://github.com/alalek/opencv/wiki/DNN-Efficiency`

### 为什么第一版不选 Pose

MediaPipe Pose、MoveNet、BlazePose 等模型可以在现代桌面设备上实时运行，但它们输出人体关键点，解决的是姿态估计问题。

当前需求只需要：

```text
有没有 person bbox
person bbox 是否离开画面
```

用 pose 会带来额外运行时、模型集成和状态解释成本。除非后续需要判断坐姿、朝向、是否背对屏幕，否则 pose 不应作为第一版默认方案。

参考：

- MediaPipe Pose：`https://github.com/google-ai-edge/mediapipe/blob/master/docs/solutions/pose.md`
- MoveNet：`https://github.com/tensorflow/tfjs-models/blob/master/pose-detection/src/movenet/README.md`

### 为什么第一版不默认选 YOLO

YOLOv8n/YOLO11n 等 nano 模型准确率通常优于老的 MobileNet-SSD，但第一版默认使用 YOLO 有几个成本：

1. 常见输入为 640x640，单次推理更重。
2. ONNX 输出解析和 NMS 后处理更复杂。
3. OpenCV DNN 跑 YOLO ONNX 需要注意 OpenCV 版本和输出格式。
4. 如果使用 OpenVINO 提速，需要额外 runtime、模型转换和打包策略。

YOLO 不是否定项，而是第二阶段对比项。第一版先用 MobileNet-SSD 建立 person bbox pipeline 和 benchmark。若准确率不够，再用同一套接口替换为 YOLOv8n + OpenVINO。

参考：

- OpenVINO YOLOv8 notebook：`https://docs.openvino.ai/2024/notebooks/yolov8-object-detection-with-output.html`
- Frigate detectors：`https://github.com/blakeblackshear/frigate/blob/v0.17.1/docs/docs/configuration/object_detectors.md`

## 运行模式选择

### 不推荐：10 秒一次单帧检测

10 秒一次只能判断：

```text
上一轮有人
十秒后没人
```

它无法捕捉用户起身、向边界移动、离开画面的过程。用户从站起来到离开摄像头范围可能只需要 1-3 秒，因此单纯低频采样不适合离开轨迹检测。

### 不推荐默认：load-per-sample

`load-per-sample` 指每次采样时加载模型、打开摄像头、执行检测，然后释放模型和摄像头。

优点：

```text
空闲内存最低
摄像头不长期打开
```

缺点：

```text
每次检测有模型加载和摄像头 warm-up 延迟
检测时机不稳定
不适合连续 bbox 轨迹判断
```

这里的“抖动”不是 UI 卡顿，而是检测延迟和响应时机抖动。例如某次检测可能 200ms 完成，另一次可能因为模型或摄像头初始化花 1-2 秒。对于离开轨迹检测，这种延迟可能错过关键帧。

### 推荐默认：continuous-low-fps

默认推荐常驻 person detector 和摄像头，但低 FPS 检测。

建议参数：

```text
PresenceTrackingMode = continuous-low-fps
PresenceDetectorFps = 2 或 3
PresenceUnloadModelWhenIdle = false
```

语义：

```text
模型常驻
摄像头持续打开
每秒检测 2-3 次
只保留 bbox 和状态
不保存帧
不上传画面
```

这个模式比 10 秒采样更能捕捉离开过程，也比 30 FPS 实时分析更省资源。

## 资源预期

以下数值需要后续 benchmark 验证，不作为最终承诺。

| 模式 | Working Set 预期 | CPU 预期 | 说明 |
| --- | ---: | ---: | --- |
| service 空闲 | 20-30 MB | 接近 0 | VM 当前实测约 23.6 MB |
| 当前 face presence running | 70-130 MB | 低到中 | 已观察到过约 70-120 MB |
| MobileNet-SSD 常驻 2-3 FPS | 50-100 MB | 低 | 第一版推荐验证 |
| YOLOv8n CPU 2-3 FPS | 80-200 MB | 中 | 可能更准，但更重 |
| YOLOv8n + OpenVINO | 100 MB 以上可能 | 低到中 | 性能好，部署复杂 |

如果 MobileNet-SSD 常驻低 FPS 的运行内存低于或接近当前 face presence，且 CPU 平均可控，则不应使用 `load-per-sample`。

## Person Bbox 状态机

新增 observation：

```text
HumanPresent {
  bbox,
  confidence,
  roi_overlap_ratio,
  bbox_area_ratio,
  center_x,
  center_y
}

HumanDepartureSuspect {
  movement_direction,
  boundary_distance_ratio,
  bbox_area_delta_ratio
}

HumanAbsent

CameraUnavailable
```

建议状态机：

```text
StableHumanPresent
  person bbox 稳定存在于座位 ROI。

DepartureSuspect
  bbox 连续向边界移动、面积变小，或 ROI overlap 下降。

AbsentCandidate
  未检测到 person bbox，但还没有达到锁屏帧数。

LockRequested
  连续缺失或确认离开后锁屏。
```

锁屏策略：

```text
StableHumanPresent:
  不锁屏。

DepartureSuspect:
  提高检测频率或保持当前 2-3 FPS。

HumanAbsent 连续 N 帧:
  锁屏。

检测到 person bbox 恢复:
  回到 StableHumanPresent。
```

## Bbox 离开判定

需要维护一个短窗口，例如最近 5-10 个 bbox。

核心指标：

```text
center_delta_x
center_delta_y
bbox_area_delta
distance_to_nearest_boundary
roi_overlap_ratio
missing_frame_count
```

离开判定示例：

```text
if missing_frame_count >= PresenceAbsentRequiredFrames:
    lock

if center 连续向最近边界移动
   and boundary_distance_ratio <= PresenceBoundaryMarginRatio:
    departure_suspect

if bbox_area_ratio 连续下降
   and roi_overlap_ratio 连续下降:
    departure_suspect

if departure_suspect 后 person bbox 消失:
    lock
```

必须避免“静坐不动”被误判：

```text
bbox 稳定且仍在 ROI 内 -> HumanPresent
没有显著移动但 person 存在 -> HumanPresent
```

## 配置开关

### 总开关

默认安装和本地摄像头认证配置不启用离开锁屏。以下是显式开启 person presence lock 时的配置示例：

```text
PresenceLockEnabled=true
PresenceDetectorKind=opencv-dnn-person
PresenceTrackingMode=continuous-low-fps
PresencePersonDetectorModel=yolov8-onnx
PresenceUnloadModelWhenIdle=false
```

可选值：

```text
PresenceDetectorKind:
  face-owner-match
  opencv-dnn-person

PresenceTrackingMode:
  face-policy
  continuous-low-fps

PresencePersonDetectorModel:
  mobilenet-ssd
  yolov8-onnx
```

### 频率和窗口

```text
PresenceDetectorFps=2
PresencePersonSuspectFps=5
PresenceAbsentRequiredFrames=6
```

当前运行策略是自适应 FPS：稳定有人时按 `PresenceDetectorFps` 检测；检测到 bbox 靠近边界、朝边界移动或面积明显缩小后，或已经进入 person absent 疑似阶段时，临时切换到 `PresencePersonSuspectFps`。重新稳定后回到低 FPS。

低视角/窄视角摄像头可能拍不到完整离开轨迹，因此策略还必须有保底：连续检测到 person 达到确认阈值后，即使没有捕捉到靠边、移动或面积缩小，只要后续连续 absent 达到 `PresenceAbsentRequiredFrames`，也按 `PersonLeftFrame` 请求锁屏。单次偶发 person 误检不会建立这个保底状态。

### Bbox 判定

```text
PresencePersonConfidenceThreshold=0.55
PresenceBoundaryMarginRatio=0.15
PresenceMovementDeltaRatio=0.08
```

`PresenceMovementDeltaRatio` 同时用于横向移动阈值和 bbox 面积缩小阈值。真实离开实验里，人未明显靠近画面边界，但 bbox 面积快速缩小后连续 absent，因此面积缩小必须作为 departure evidence。

### 人脸增强

```text
PresenceFaceOwnerAssistEnabled=true
PresenceOwnerMatchThreshold=0.50
PresenceUnknownFaceAuditEnabled=true
```

含义：

```text
person bbox 存在但脸不可见 -> 不锁
face owner match 通过 -> 强化 HumanPresent
unknown face 连续出现 -> 可选审计或加速锁屏
```

### 隐私与审计

```text
PresenceSaveCameraFrames=false
PresenceSaveUnknownFaceCrop=false 或 true
PresenceSaveScreenSnapshot=false 或 true
PresenceAuditMaxRecordCount=50
```

默认不保存摄像头完整帧。若开启审计，必须单独配置。

## 模块设计

建议新增抽象：

```rust
pub trait PresenceDetector {
    fn observe_presence(&mut self, frame: &Frame) -> Result<PresenceDetection, PresenceDetectorError>;
}

pub enum PresenceDetection {
    HumanPresent(PersonDetection),
    HumanAbsent,
    CameraUnavailable,
}

pub struct PersonDetection {
    pub bbox: NormalizedRect,
    pub confidence: f32,
}
```

`presence_policy` 不应直接依赖 OpenCV DNN 或具体模型。它只消费结构化 observation。

建议新增模块：

```text
presence_detector
  trait 和通用 detection 类型。

presence_person_detector_opencv
  OpenCV DNN MobileNet-SSD/SSDLite 实现。

presence_bbox_tracker
  维护 bbox 短窗口，计算位移、边界距离和消失帧。

presence_runtime_config
  解析 registry 配置，避免配置逻辑继续堆在 service_config。
```

## Benchmark 计划

在正式接入 service 默认路径前，先做 diagnostics benchmark。

建议命令：

```text
diagnostics_cli.exe presence-person-benchmark ^
  --detector mobilenet-ssd ^
  --fps 2 ^
  --duration-seconds 120 ^
  --camera-id 0
```

输出：

```text
detector_kind
fps_target
sample_count
person_detected_count
inference_ms_p50
inference_ms_p95
working_set_mb_before
working_set_mb_peak
working_set_mb_after
private_mb_peak
cpu_percent_avg
bbox_jitter_score
```

验收门槛建议：

```text
working_set_mb_peak <= 100
cpu_percent_avg <= 5
inference_ms_p95 <= 150
person_detected_rate 在正常坐姿下稳定
转头/低头不触发 absent
离开画面后 2-5 秒内锁屏
```

MobileNet-SSD 已作为 baseline/fallback 保留；当前本机验证显示 YOLOv8n ONNX 的 person 检出稳定性明显更好，正式 person 路线优先配置：

```text
PresencePersonDetectorModel=yolov8-onnx
PresencePersonModelPath=models\yolov8n.onnx
PresencePersonModelConfigPath=<none>
```

## 推进路线

### Phase 1：配置和 Benchmark

1. 补齐 presence registry 配置。
2. 增加 `PresenceDetectorKind`、`PresenceTrackingMode`、`PresenceDetectorFps` 等开关。
3. 新增 `presence-person-benchmark` 诊断命令。
4. 在 VM 中记录 YOLOv8n 常驻 2 FPS 与疑似态 5 FPS 的内存、CPU 和延迟。

交付标准：

```text
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
VM benchmark 报告
```

### Phase 2：Person Bbox Detector 接入

1. 新增 `PresenceDetector` trait。
2. 新增 OpenCV DNN MobileNet-SSD/SSDLite detector。
3. 新增 bbox tracker。
4. `presence_policy` 改为消费 `HumanPresent`、`HumanAbsent`、`DepartureSuspect`。
5. service 默认仍可关闭或使用旧 face 模式，避免一次性替换风险。

交付标准：

```text
检测到 person bbox 时，转头/低头不锁屏
person bbox 离开画面后触发锁屏
连续缺失帧防抖有效
VM 上持续运行 10 分钟资源可控
```

### Phase 3：策略收敛

1. 默认 detector 从 face-only 切到 person bbox。
2. face owner match 降级为辅助信号。
3. 加入 ROI 配置和调试可视化输出。
4. 记录误锁样本的非图像元数据，例如 bbox 轨迹和状态转换。

### Phase 4：更高精度模型对比

若 MobileNet-SSD 漏检或误检明显，再评估：

```text
YOLOv8n + OpenVINO
YOLOv8n + OpenCV DNN
MoveNet / MediaPipe Pose
```

只有 benchmark 明确证明收益大于部署和资源成本时，才切换默认模型。

### Phase 5：外部传感器联动

后续接入 24G 毫米波或小车视频源时，可让外部传感器作为低功耗 presence gate：

```text
雷达检测有人 -> 摄像头低 FPS person bbox 验证
雷达检测无人 -> 摄像头可关闭或降低频率
```

这会比单摄像头常开更隐私友好，但不进入当前阶段。

## 决策摘要

当前建议：

```text
默认目标：person bbox presence
第一模型：OpenCV DNN MobileNet-SSD/SSDLite
默认运行：continuous-low-fps，稳定态 2 FPS，疑似离开态 5 FPS
默认资源策略：模型常驻，不 load-per-sample
默认隐私策略：不保存摄像头帧，只保存 bbox 状态
升级路径：benchmark 不达标时再评估 YOLOv8n/OpenVINO
```

这个路线的核心取舍是：

```text
用轻量 person detector 替代 face-only presence
用低 FPS 常驻替代 10 秒低频采样
用 bbox 轨迹判断离开画面
用配置开关保留低资源和高准确两条路径
```
