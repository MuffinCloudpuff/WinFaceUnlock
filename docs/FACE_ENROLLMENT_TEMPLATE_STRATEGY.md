# 引导式人脸注册与模板集策略

更新时间：2026-06-01

## 目标

本项目的人脸注册不应要求用户手动分类照片，也不应只保存一张正脸模板。目标是做成类似手机的人脸录入体验：

```text
用户按提示录一小段
-> 系统自动抽帧
-> 自动判断质量
-> 自动估计姿态和动作
-> 自动筛选代表帧
-> 生成结构化模板集
-> 登录时按模板集匹配
```

这部分是提高识别效果的核心，不只是模型接入问题。模型能跑通只能证明链路可用；真正影响登录体验的是注册样本质量、姿态覆盖、模板组织方式、阈值校准和认证策略。

## 设计原则

1. 用户只按提示动作，不手动分类。
2. 系统内部保留多模板，不把所有 embedding 粗暴平均成一个向量。
3. 注册样本必须经过质量筛选，低质量样本只进入诊断日志，不参与解锁。
4. 姿态分类优先采用“引导步骤标签 + 自动校验”，不依赖纯自动分类。
5. 模板必须绑定检测模型和识别模型版本，模型变更后旧模板不能静默复用。
6. 第一版先做 CLI 和 diagnostics 闭环，不急着做 UI。
7. 登录链路必须保留 PIN/密码兜底；人脸模板质量差时只能导致本模块拒绝认证，不能影响系统登录。

## 为什么不能只存一张照片

单张正脸照的优点是分数通常高、注册简单。但真实登录时，用户可能是：

- 笔记本摄像头偏低，脸有俯仰角。
- 身体坐偏，脸有轻微左右转角。
- 光线从窗户背后照进来。
- 佩戴眼镜，有反光。
- USB 摄像头像素低，检测框和关键点抖动。

如果只保存一张正脸模板，系统很容易出现：

- 正脸能过，稍微侧一点就不过。
- 注册照分数高，真实摄像头分数低。
- 阈值调低能过，但误接收风险增加。
- 无法解释失败原因，只能看到一个最终分数。

因此本项目应保存一个 `FaceTemplateSet`，而不是单个 `FaceTemplate`。

## 用户体验流程

第一版推荐使用引导式注册，总时长控制在 12 到 20 秒：

```text
1. 正脸看摄像头，保持 2 秒
2. 缓慢向左转头
3. 回到正脸
4. 缓慢向右转头
5. 回到正脸
6. 稍微低头
7. 稍微抬头
8. 眨眼一次或两次
9. 系统生成模板质量报告
```

用户看到的是简单提示；系统内部每一帧都会被自动打分和归类。

### 为什么采用“引导步骤标签 + 自动校验”

纯自动分类难度高，因为系统需要在任意视频帧里判断用户是正脸、左转、右转、低头、抬头还是动作过渡。

更稳的策略是：

```text
当前步骤提示：请缓慢向左转头
-> 这段帧默认候选标签为 yaw_left
-> 后台估计 yaw/pitch/roll
-> 如果姿态确实符合 yaw_left，且质量足够，则进入 yaw_left 候选池
-> 如果姿态不符合或质量差，则丢弃或标记为 rejected
```

这样系统不是从零猜分类，而是在已知用户当前动作意图的前提下做校验，鲁棒性更高，也更容易解释失败原因。

## 模板池结构

推荐结构：

```text
FaceTemplateSet
  user_id
  detector_model_id
  recognizer_model_id
  enrollment_id
  enrollment_created_at
  primary_threshold_profile
  samples[]
  pose_groups[]
  quality_summary
```

### Pose Group

第一版定义以下模板组：

| 组名 | 作用 | 默认是否参与解锁 |
|---|---|---|
| `frontal_primary` | 主正脸模板，阈值基准 | 是 |
| `frontal_variant` | 正脸不同距离、光照、轻微表情 | 是 |
| `yaw_left_mild` | 轻微左转 | 是 |
| `yaw_right_mild` | 轻微右转 | 是 |
| `pitch_down_mild` | 轻微低头 | 是 |
| `pitch_up_mild` | 轻微抬头 | 是 |
| `blink_motion` | 眨眼动作证据 | 默认不作为身份模板 |
| `hard_pose_diagnostic` | 大角度侧脸或困难姿态 | 默认不参与 |
| `rejected_quality` | 质量不合格样本 | 不参与 |

第一版不要支持太多细分类。模板组过多会让注册流程变长，也会让阈值校准变复杂。

### Sample Metadata

每个候选样本保存以下元数据：

```text
sample_id
pose_group
source_step
frame_timestamp_ms
face_box
landmarks
aligned_face_ref
embedding_ref
quality_score
blur_score
illumination_score
face_size_score
alignment_score
pose_yaw_deg
pose_pitch_deg
pose_roll_deg
detection_confidence
recognition_model_id
detector_model_id
selected_for_unlock
reject_reason
```

注意：

- `quality_score` 表示该样本是否适合作为识别模板。
- `selected_for_unlock` 表示是否进入实际认证模板池。
- `reject_reason` 必须是结构化枚举，不用模糊字符串。

建议枚举：

```text
FaceSampleRejectReason
  NoFaceDetected
  MultipleFacesDetected
  FaceTooSmall
  FaceTooLargeOrClipped
  BlurTooHigh
  UnderExposed
  OverExposed
  BacklightTooStrong
  LandmarkUnstable
  PoseOutOfExpectedRange
  AlignmentFailed
  EmbeddingInconsistentWithPrimary
  DuplicateTooSimilar
```

## 质量评分

第一版不需要直接上复杂 FIQA 模型。可以先用可解释、可实现的基础质量分：

```text
quality_score =
  face_size_score
  + sharpness_score
  + exposure_score
  + landmark_stability_score
  + pose_fit_score
  + alignment_score
  + embedding_consistency_score
```

### 基础指标

| 指标 | 说明 | 第一版实现方式 |
|---|---|---|
| face size | 人脸在画面中是否足够大 | face box 像素面积、眼间距 |
| blur | 是否模糊 | Laplacian variance |
| exposure | 是否过曝/欠曝 | 灰度直方图、均值、饱和比例 |
| landmark stability | 关键点是否稳定 | 连续帧关键点抖动 |
| pose fit | 是否符合当前引导步骤 | yaw/pitch/roll 范围 |
| alignment | 对齐 crop 是否正常 | align_crop 成功、关键点几何关系 |
| embedding consistency | 是否仍然像同一个人 | 与正脸主模板分数比较 |

### 质量分用途

质量分不直接等于认证分数。它只用于决定：

- 这帧是否适合保存。
- 同一个姿态组里选择哪几帧。
- 是否提示用户重新录某一步。
- 后续诊断为什么注册质量差。

## 姿态估计策略

### 第一版：YuNet 五点关键点估算

当前 YuNet 可以提供五点关键点。第一版可以先用五点关键点估计粗略姿态：

- 双眼中心线斜率估计 roll。
- 鼻尖相对双眼中心和嘴角中心的位置估计 yaw/pitch 倾向。
- 左右眼到鼻尖距离差估计 yaw 倾向。
- 鼻尖到嘴部距离、眼鼻比例估计 pitch 倾向。

这不是精确 3D head pose，但足够做第一版粗分类校验。

### 第二版：MediaPipe Face Landmarker

如果五点关键点不够稳定，再引入 MediaPipe Face Landmarker：

- 输出更密集的 3D 人脸关键点。
- 可辅助估计 yaw/pitch/roll。
- 可检测眨眼、表情和头部动作。
- 用于注册可视化和质量诊断。

MediaPipe 不作为身份识别模型，只做姿态、质量和动作辅助。

## 注册样本选择策略

每个步骤采集多帧，但最终只保存少量高质量模板。

推荐初始数量：

| 模板组 | 目标保留数量 |
|---|---:|
| `frontal_primary` | 2 |
| `frontal_variant` | 2 |
| `yaw_left_mild` | 1 到 2 |
| `yaw_right_mild` | 1 到 2 |
| `pitch_down_mild` | 1 |
| `pitch_up_mild` | 1 |

总模板数控制在 8 到 10 个。这样既覆盖真实姿态，又不会让匹配成本和误接收面过大。

### 去重

同一个姿态组里，如果两个样本 embedding 非常接近，只保留质量更高的一个：

```text
if similarity(sample_a, sample_b) > duplicate_similarity_threshold:
  keep sample with higher quality_score
```

### 主模板

`frontal_primary` 必须优先建立。后续所有非正脸模板都需要和主模板做一致性校验：

```text
if compare(non_frontal_embedding, frontal_primary_embedding) < enrollment_consistency_threshold:
  reject EmbeddingInconsistentWithPrimary
```

这能避免把别人入镜、误检脸或极差帧保存进模板池。

## 登录匹配策略

登录时不做模板平均，先采用 max score：

```text
live_embedding
-> compare with all selected templates
-> best_template = max(score)
-> best_score >= threshold
-> 连续 N 帧中 M 帧通过
-> 生成短时授权 grant
```

认证结果必须记录：

```text
best_score
best_template_id
best_pose_group
frontal_best_score
non_frontal_best_score
frames_observed
frames_passed
auth_match_passed
grant_issued
```

不要只返回 `success: true`。不同层级的判定必须命名清楚。

## 阈值策略

第一版阈值分为三层：

```text
template_acceptance_threshold
  注册阶段：非正脸模板必须和主模板足够相似

frame_match_threshold
  登录阶段：单帧和模板集的最高匹配分需要达到阈值

sequence_auth_threshold
  登录阶段：连续帧策略，决定是否发放 grant
```

示例：

```text
frame_match_threshold = 0.75
min_passed_frames = 3
max_observed_frames = 8
min_quality_score = 0.70
```

这些值只是初始值。正式值必须由 `face-calibrate` 根据本机摄像头和注册模板生成。

## CLI 设计

第一版先做 diagnostics CLI，不做 UI。

### guided-enroll

建议命令：

```powershell
diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment `
  --frames-per-step 60 `
  --save-debug-images
```

交互提示：

```text
[1/6] 请正脸看摄像头
[2/6] 请缓慢向左转头
[3/6] 请缓慢向右转头
[4/6] 请稍微低头
[5/6] 请稍微抬头
[6/6] 请眨眼一次
```

当前实现已经改为手机式状态机，而不是单纯按时间推进：

```text
进入某一步
-> waiting_for_pose：持续检测当前姿态
-> 连续 N 帧姿态和质量达标
-> recording_started：开始保存该步骤候选样本
-> 采够 accepted_frames_per_step
-> 自动进入下一步
```

也就是说，如果提示“请缓慢向左转头”，系统会先用 `FacePoseProvider` 判断当前帧是否真的符合左转；未达标时不会进入该步骤录制。录制期间如果姿态又偏离，也不会计入该步骤样本。

当前参数：

```powershell
diagnostics_cli.exe guided-enroll `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --output-dir .\face-enrollment `
  --accepted-frames-per-step 6 `
  --max-wait-frames-per-step 180 `
  --max-frames-per-step 180 `
  --pose-ready-consecutive 3 `
  --pose-ready-min-fit 0.25 `
  --save-debug-images
```

`pose-ready-min-fit` 当前默认是 `0.25`，原因是 `landmark` fallback 只用 YuNet 五点关键点粗估姿态，分数不应设得过高。等 MediaPipe Face Landmarker bridge 可用后，可以把这个值提高，再让眨眼步骤参与完整注册流程。

调试阶段如果某个姿态步骤一直无法完成，但希望先使用已采集到的正脸、左转或部分右转样本，可以显式加：

```powershell
--allow-partial-enrollment
```

启用后，未完成的步骤不会让整个命令失败，CLI 会继续生成：

```text
selected_templates.json
enrollment_report.json
partial_enrollment_reasons.json
```

这个模式只适合实验和调参，不应作为正式注册默认行为。正式使用仍应要求关键步骤完整通过，避免模板集姿态覆盖不足。

输出：

```text
enrollment_report.json
selected_templates.json
debug_frames/
aligned_faces/
rejected_samples.jsonl
```

### enrollment-report

建议命令：

```powershell
diagnostics_cli.exe enrollment-report `
  --user-id dev-user `
  --enrollment-id latest
```

输出：

```text
模板总数
各 pose_group 模板数
平均质量分
最低质量分
被拒绝原因统计
推荐是否重新注册
```

### face-auth-debug

认证调试命令需要展示每帧命中哪个模板组：

```powershell
diagnostics_cli.exe face-auth-debug `
  --user-id dev-user `
  --camera-id opencv-index:0 `
  --frames 30 `
  --save-debug-images
```

输出字段：

```text
frame_index
best_score
best_template_id
best_pose_group
quality_score
auth_match_passed
reject_reason
```

## 数据存储边界

### Credential Store

长期保存：

- 模板 metadata。
- embedding 加密引用。
- 模型 identity。
- 质量摘要。
- 阈值 profile。

不长期保存：

- 原始摄像头视频。
- 大量原始帧。
- 不必要的清晰人脸照片。

### Debug 输出

debug 图像只在用户主动运行 diagnostics 时输出，并写到项目或指定目录。Service 自动登录链路默认不保存原始人脸图像。

日志中不得记录：

- Windows 密码。
- 明文凭据。
- 完整 embedding 向量。
- 大量可识别人脸图像路径，除非是显式 diagnostics。

## 与现有模块的边界

推荐模块职责：

```text
face_pose
  位姿检测统一接口；输出 yaw/pitch/roll、眨眼/动作等注册辅助信号。

face_pose_mediapipe
  后续 MediaPipe Face Landmarker adapter；只实现 face_pose trait，不进入身份识别主链路。

face_quality
  质量评分、拒绝原因、抽帧选择

face_enrollment
  引导流程、模板池生成、注册报告

face_templates
  模板数据结构、模板选择、模型 identity 校验

face_auth
  登录时多模板匹配、连续帧策略、认证结果

diagnostics_cli
  注册、报告、调试命令
```

不要把质量评分、姿态估计、模板保存和登录策略都写进一个文件。

更具体的运行时编排边界见 `docs/FACE_MODULE_COMPOSITION.md`。当前代码已经把 `face_auth` 拆成认证核心和 `enrollment` feature：`win_service` 关闭默认 feature，只依赖认证核心；`diagnostics_cli` 才启用注册、位姿和质量评分能力。

### 位姿模块边界

位姿检测必须作为独立模块，不和 YuNet/SFace 身份识别绑定。

当前实现允许两类后端：

```text
LandmarkFacePoseProvider
  临时 fallback。
  基于 YuNet 五点关键点估算 yaw/pitch/roll。
  只用于早期调试，不作为长期最优方案。
  该 provider 不声明 blink 能力，注册流程不能用它伪造眨眼通过。

MediaPipeFacePoseProvider
  主推增强路线。
  当前 Rust 侧由 `face_pose_mediapipe` crate 通过动态 DLL bridge 接入。
  基于 MediaPipe Face Landmarker 的 3D face landmarks / transformation matrix / blendshapes。
  用于更稳定判断左转、右转、低头、抬头和眨眼。
```

`face_auth` 的 guided enrollment 只依赖统一 trait：

```text
FacePoseProvider::estimate_pose(frame, detected_face) -> FacePoseEstimate
```

因此后续从 fallback 切到 MediaPipe 时，不应改注册模板结构、不应改 SFace 识别逻辑，也不应影响 Windows 登录链路。

## 实施顺序

### Phase A：文档与数据结构

1. 固化 `FaceTemplateSet`、`FaceTemplateSample`、`FacePoseGroup`、`FaceSampleRejectReason`。
2. 给模板记录增加 detector/recognizer model identity。
3. 给认证结果增加 best template 和 pose group 字段。

### Phase B：离线/CLI 注册

1. 实现 `guided-enroll`。
2. 实现基础质量评分。
3. 实现引导步骤标签和独立 `FacePoseProvider` 位姿校验。
4. 实现每组 top-k 样本选择。
5. 输出注册报告和 debug 图。

### Phase B.5：MediaPipe 位姿增强

1. 新增 `face_pose_mediapipe` adapter。
2. 使用 MediaPipe Face Landmarker 输出 yaw/pitch/roll 和眨眼信号。
3. 保留 `LandmarkFacePoseProvider` 作为 fallback。
4. 在 diagnostics CLI 中增加 `--pose-provider landmark|mediapipe`。
5. 用同一套注册动作对比两个 provider 的完成率和误拒原因。

### Phase C：多模板认证

1. Service 读取模板集。
2. 登录时对所有启用模板取 max score。
3. 输出 best template、best group、连续帧状态。
4. 用 diagnostics 验证不同姿态下命中情况。

### Phase D：校准与模型评估

1. 用 `face-calibrate` 统计每个模板组的分数分布。
2. 输出推荐阈值。
3. 对 YuNet/SFace baseline 建立报告。
4. 再评估 MediaPipe 姿态辅助或 ArcFace 替代路线。

## 验收标准

第一版合格标准：

1. 用户不需要手动分类样本。
2. 注册流程能输出结构化模板集。
3. 每个入选模板都有质量分、姿态组和模型 identity。
4. 低质量样本会被拒绝，并给出结构化原因。
5. 登录时能显示命中哪个模板组。
6. 正脸、轻微左转、轻微右转、轻微低头、轻微抬头都能产生可解释的分数报告。
7. 模板模型不匹配时拒绝认证并提示重新注册。
8. 不保存不必要的原始视频和大量人脸图像。

## 参考项目和资料

- InsightFace Evaluation Studio：本地 1:1 / 1:N 评估、identity-folder 数据集、DBSCAN 聚类、阈值和报告输出。
- DeepFace：支持多张注册图、可切换 detector/recognizer/distance metric/threshold，并强调 detect、align、normalize、represent、verify 完整链路。
- IJB-A / IJB-C 评测体系：使用 multi-image template，强调多媒体模板、聚类、模板创建和低 FAR 评估。
- SER-FIQ / MagFace / OFIQ：人脸图像质量评估方向，可作为后续质量评分增强参考。
- MediaPipe Face Landmarker：适合姿态、关键点、眨眼和可视化辅助，不作为身份识别主模型。
