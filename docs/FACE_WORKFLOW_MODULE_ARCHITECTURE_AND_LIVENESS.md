# 人脸流程编排、模块边界与防视频攻击设计

更新时间：2026-06-01

## 目标

本项目的人脸能力不应做成一条写死的大链路，而应拆成可插拔模块，再由不同 workflow 按需编排。这样后续新增防视频攻击、替换识别模型、替换位姿模型、增加雷达或深度摄像头时，不需要推倒登录主链路。

核心原则：

```text
模块提供能力
workflow 决定顺序
策略决定是否通过
Service / Provider 只调用稳定 workflow
```

## 当前模块能力

### video_provider

职责：

- 枚举摄像头。
- 打开摄像头。
- 读取 `VideoFrame`。

不负责：

- 人脸检测。
- 人脸识别。
- 活体判断。

### face_engine

职责：

- 人脸检测。
- 人脸关键点输出。
- 对齐 crop。
- 提取识别 embedding。
- 模板相似度比较。

当前实现：

```text
检测：OpenCV Zoo YuNet
识别：OpenCV Zoo SFace
```

边界：

- 检测模型和识别模型必须独立可替换。
- `SFACE_COSINE_MATCH_THRESHOLD = 0.363` 保留为 OpenCV/SFace 原始参考阈值。
- 项目登录策略阈值当前默认是 `0.75`，不要和模型参考阈值混淆。

### face_pose

职责：

- 估计 yaw / pitch / roll。
- 后续支持 blink score。
- 为注册引导和活体挑战提供姿态信号。

当前实现：

```text
LandmarkFacePoseProvider
  基于 YuNet 五点关键点粗估姿态。
  可用于正脸、左右转头、低头、抬头的粗判断。
  不支持眨眼。

MediaPipeFacePoseProvider
  Rust adapter 和 ABI 已定义。
  目标是通过 MediaPipe Face Landmarker 输出更稳定的姿态和眨眼信号。
  当前 native bridge 构建仍待完成。
```

### face_auth

职责：

- 登录身份认证策略。
- 多模板匹配。
- 连续帧通过策略。
- 注册模板筛选和质量报告。

边界：

- `FaceAuthenticator` 负责“像不像已注册用户”。
- 不应把活体检测、屏幕检测、摄像头采集直接混进 `FaceAuthenticator`。
- 活体应作为独立模块，由上层 workflow 组合。

### face_liveness（已新增独立模块）

职责：

- 判断当前摄像头画面是否像实时真人，而不是照片、屏幕或视频回放。
- 输出结构化 `LivenessResult`。
- 可由多个 provider 组合。

第一版已实现 provider：

```text
ScreenReplayLivenessProvider
  使用灰度图 -> THRESH_BINARY_INV -> inRange(0, 50) 的预处理 mask。
  基于 RETR_EXTERNAL 找最外层轮廓，检测画面里是否存在手机/平板/显示器播放攻击特征。
```

后续计划 provider：

```text
PoseChallengeLivenessProvider
  后续基于眨眼、转头、低头抬头等动作挑战。

AntiSpoofModelLivenessProvider
  后续可接 ONNX anti-spoof 模型。
```

### credential_store

职责：

- 保存用户、凭据引用、模板记录、策略记录。
- 保存默认安全策略。

当前默认人脸匹配阈值已经收紧到：

```text
face_match_threshold = 0.75
```

### win_service

职责：

- 在 Windows Service 中承载认证 workflow。
- 读取 Service 配置。
- 根据认证结果签发短期 grant。

边界：

- 不直接实现检测、识别、活体算法。
- 只组合稳定模块。

### windows_provider

职责：

- Credential Provider / LogonUI 交互。
- 唤醒 Service。
- 提交凭据。

边界：

- 不做模型推理。
- 不直接读摄像头。
- 不保存人脸图像。

## Workflow 编排

### 注册 workflow

当前注册流程：

```text
摄像头
-> 人脸检测
-> 位姿检测
-> 质量评分
-> embedding 提取
-> 模板筛选
-> 输出 selected_templates.json / enrollment_report.json
```

当前 `guided-enroll` 已经是状态机：

```text
进入某一步
-> waiting_for_pose
-> 连续 N 帧姿态和质量达标
-> recording_started
-> 每帧继续做位姿和质量筛选
-> 合格帧进入候选模板池
-> 每组 top-k 选出最终模板
```

### 登录 workflow 当前版本

当前登录链路：

```text
摄像头
-> 人脸检测
-> embedding 提取
-> 模板匹配
-> 连续帧策略
-> 签发 grant
```

当前它主要解决“是不是同一个人”。

### 登录 workflow 加活体后的目标版本

用户当前倾向的插入顺序：

```text
摄像头
-> 人脸检测
-> 活体检测
-> embedding 提取
-> 模板匹配
-> 连续帧策略
-> 签发 grant
```

这个顺序的优点：

- 活体失败时可以尽早拒绝，不浪费 embedding 和模板匹配成本。
- 防视频攻击可以先看整幅画面，不依赖识别结果。
- 不影响后面的识别模块，仍然保持模块解耦。

注意：

- 如果某类活体检测需要确认“这是目标用户”后再挑战，也可以放在模板匹配之后。
- workflow 应允许不同 `LivenessProvider` 声明自己适合放在 `pre_recognition` 还是 `post_recognition`。

## 防视频攻击第一版：屏幕回放检测

### 设计直觉

手机、平板、显示器播放人脸视频时，摄像头画面里常见一个明显的电子屏幕区域。这个区域通常有以下特征：

- 亮度分布和自然环境不同。
- 屏幕边界接近矩形。
- 屏幕内部可能出现大面积过亮或过暗区域。
- 屏幕区域和周围背景有明显边界。
- 人脸出现在这个矩形区域内部。

因此第一版可以先做一个轻量的 `ScreenReplayLivenessProvider`：

```text
整帧图像
-> 灰度/亮度分析
-> 二值化或自适应阈值
-> 轮廓检测
-> 找近似矩形的大区域
-> 判断人脸框是否落在该矩形内
-> 如果命中，拒绝后续识别
```

### 输入

```text
VideoFrame
DetectedFace
可选：FaceBox / landmarks
```

### 输出

建议定义：

```rust
pub struct LivenessResult {
    pub liveness_decision: LivenessDecision,
    pub liveness_score: Option<f32>,
    pub evidence: Vec<LivenessEvidence>,
}

pub enum LivenessDecision {
    LiveAccepted,
    SpoofRejected,
    Inconclusive,
    ProviderUnavailable,
}

pub enum LivenessEvidence {
    ScreenLikeRectangleDetected {
        rectangle: FaceImageRect,
        face_inside_rectangle: bool,
        rectangle_area_ratio: f32,
        brightness_contrast_score: f32,
        edge_rectangularity_score: f32,
    },
    NoScreenLikeRectangleDetected,
}
```

命名必须明确，避免使用模糊的 `success: true`。

### 第一版算法草案

```text
1. 将整帧转灰度或亮度通道。
2. 做高亮/低亮区域分割。
3. 做边缘检测或轮廓提取。
4. 找面积足够大的四边形/近似矩形。
5. 过滤掉太小、太窄、太靠边或不像屏幕比例的区域。
6. 判断人脸框是否完全位于该矩形内，或人脸框与屏幕候选区域的重叠率是否足够高。
7. 如果矩形区域明显且人脸在内部，返回 SpoofRejected。
8. 如果没有可靠证据，返回 Inconclusive，不直接当作通过。
```

### 推荐阈值字段

```text
min_screen_area_ratio
max_screen_area_ratio
min_rectangularity_score
min_brightness_contrast_score
min_face_inside_screen_ratio
```

示例初始值只用于调试，不能直接当最终安全参数：

```text
min_screen_area_ratio = 0.12
max_screen_area_ratio = 0.90
min_rectangularity_score = 0.70
min_brightness_contrast_score = 0.45
min_face_inside_screen_ratio = 0.95
```

### 这个方案能防什么

可以优先覆盖：

- 手机屏幕播放人脸视频。
- 平板屏幕播放人脸视频。
- 显示器上播放人脸视频后对着摄像头。
- 一些静态照片显示在电子屏幕上的攻击。

### 这个方案不能过度承诺

不能声称能防：

- 裁剪到只剩脸、没有屏幕边框的视频攻击。
- 攻击者把屏幕贴得很近，让边界超出摄像头画面。
- 高质量打印照片。
- 3D 面具。
- 摄像头驱动层或虚拟摄像头注入。
- 实时 deepfake。

因此第一版文案应叫：

```text
screen replay attack detection
```

不要叫完整 liveness 或完整 anti-spoof。

## 活体模块的组合策略

第一版登录 workflow 可以这样组合：

```text
摄像头读帧
-> 检测人脸
-> ScreenReplayLivenessProvider
   -> SpoofRejected: 直接拒绝，不做 embedding
   -> Inconclusive: 继续后续识别
   -> LiveAccepted: 继续后续识别
-> LivenessWindowPolicy
   -> 任一帧 SpoofRejected: 拒绝本次认证窗口
   -> 窗口内没有 SpoofRejected: 继续后续识别
-> embedding
-> 模板匹配
-> 连续帧策略
-> grant
```

为什么 `Inconclusive` 不等于失败：

- 屏幕检测第一版是启发式算法。
- 它的任务是发现明显屏幕攻击，而不是证明一定是真人。
- 如果把 `Inconclusive` 当失败，会导致大量正常场景误拒。

后续增强后可以变成：

```text
ScreenReplayLivenessProvider
-> PoseChallengeLivenessProvider
-> AntiSpoofModelLivenessProvider
-> LivenessPolicyAggregator
```

聚合策略示例：

```text
任何强证据 spoof -> 拒绝
主动挑战通过 -> live accepted
模型分数高 -> live accepted
证据不足 -> 根据配置决定 allow / deny / fallback to PIN
```

## 推荐新增 crate / 模块

已新增：

```text
crates/face_liveness/
```

目录结构：

```text
crates/face_liveness/
  src/lib.rs
  src/types.rs
  src/screen_replay.rs
  src/policy.rs
```

职责：

- `types.rs`：公共结构和枚举。
- `screen_replay.rs`：屏幕矩形检测 provider。
- `policy.rs`：多 provider 结果聚合策略。

不要把这些代码放进：

```text
face_engine
face_auth::authenticator
win_service::auth_issuer
windows_provider
```

`win_service` 后续只应该依赖 `face_liveness` 的 trait 和配置。

## diagnostics CLI 计划

已新增：

```powershell
diagnostics_cli.exe liveness-screen-debug `
  --camera-id opencv-index:0 `
  --output-dir .\liveness-debug\screen `
  --frames 60 `
  --save-debug-images
```

输出：

```text
liveness-debug/
  screen/
    annotated_frames/
    liveness_metrics.jsonl
    liveness_summary.json
```

每帧记录：

```text
frame_index
detected_face_count
screen_like_rectangle_count
best_rectangle
face_inside_rectangle
liveness_decision
liveness_score
evidence
```

这样可以先拿手机播放视频对着摄像头，验证屏幕矩形检测是否可靠。

## Service 配置计划

建议新增配置：

```text
WINFACEUNLOCK_LIVENESS_MODE
WINFACEUNLOCK_SCREEN_REPLAY_DETECTION
WINFACEUNLOCK_LIVENESS_REQUIRED
```

枚举建议：

```text
liveness_mode:
  disabled
  screen-replay-detect-only
  required
```

第一版默认：

```text
liveness_mode = disabled
```

原因：

- 当前登录主链路已经验证过，不能突然引入误拒。
- 屏幕回放检测需要先用 diagnostics 收集真实误报/漏报数据。

## 验收标准

第一阶段只验收屏幕回放检测模块，不直接要求进入正式登录。

### diagnostics 验收

1. 正常真人摄像头画面下，不应频繁输出 `SpoofRejected`。
2. 手机屏幕播放人脸视频并出现在画面中时，应稳定输出 `SpoofRejected`。
3. 手机屏幕不含人脸，只是在背景里时，不应因为背景屏幕直接误拒。
4. 输出标注图能看到检测到的屏幕矩形。
5. JSONL 能记录拒绝原因和证据。

### workflow 验收

1. `ScreenReplayLivenessProvider` 可以插在检测之后、embedding 之前。
2. 活体模块失败不会破坏摄像头、识别、模板匹配模块。
3. `Inconclusive` 和 `SpoofRejected` 语义明确。
4. 登录链路可以通过配置启用或禁用。
5. Provider / PIN / 密码保底不受影响。

## 下一步建议

1. 已新增 `face_liveness` crate 和类型定义。
2. 已实现 `ScreenReplayLivenessProvider` 的独立 diagnostics 版本。
3. 用真实手机屏幕播放视频测试：
   - 手机屏幕近距离。
   - 手机屏幕远距离。
   - 屏幕亮度高/低。
   - 黑白背景。
   - 人脸在屏幕内/不在屏幕内。
4. 输出误拒/漏报报告。
5. 再决定是否接入 `win_service` 登录 workflow。

不要在没有 diagnostics 数据前直接默认启用登录活体拦截。
