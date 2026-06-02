# 人脸识别模型路线调研记录

更新时间：2026-06-01

## 结论先行

当前 WinFaceUnlock 不建议马上替换 YuNet + SFace 主线。更合理的路线是：

1. 继续把 OpenCV YuNet + SFace 作为轻量 baseline。
2. 先实现可视化、场景采样、阈值校准和模型 A/B 评测闭环。
3. 如果数据证明 SFace 在侧脸、背光、低像素摄像头或眼镜场景下稳定性不够，再评估 InsightFace 系列的 RetinaFace/SCRFD + ArcFace。
4. MediaPipe Face Landmarker 更适合补“关键点、姿态、可视化、活体辅助”，不适合作为身份识别 embedding 的主模型。
5. 默认继续 CPU 推理；GPU/DirectML/WinML 只作为后续优化项，不进入登录链路第一优先级。

核心原因是：登录链路最重要的是稳定、可恢复、低依赖和可解释。成熟识别模型的上限更高，但如果没有本项目自己的采样和误拒/误接收数据，直接换模型只会把问题从“当前模型分数低”变成“新模型阈值和部署风险不清楚”。

## 当前项目基线

当前实现使用：

- 检测模型：OpenCV Zoo YuNet。
- 识别模型：OpenCV Zoo SFace。
- 推理接口：OpenCV DNN / OpenCV Face API。
- 运行方式：CPU 优先。
- 模块边界：检测模型和识别模型已经按 provider/pipeline 解耦，后续可以独立替换。

这条路线的优势：

- 模型小，CPU 能跑，适合登录界面这种不能拖慢系统的链路。
- OpenCV 在 Windows 桌面环境部署成熟，Rust 侧已经接入。
- YuNet 输出检测框和五点关键点，可以支撑基础对齐和可视化。
- SFace 模型文件和接口简单，适合做第一版稳定 baseline。

这条路线的风险：

- 真实摄像头下的侧脸、背光、眼镜反光、低像素 USB 摄像头效果需要本项目实测。
- 单张注册照不一定能覆盖真实登录姿态。
- 阈值不能直接照搬别的项目，必须按本项目摄像头、模型、用户数据和连续帧策略校准。
- 当前仍缺少标注图、关键点图、对齐 crop、分数分布、耗时分布等可观测输出。

## 成熟开源项目常用路线

### InsightFace / ArcFace / RetinaFace / SCRFD

调研结果显示，成熟开源人脸识别项目中，InsightFace 体系非常常见。它覆盖了人脸检测、对齐、识别、3D 分析等多个方向，识别侧包含 ArcFace、SubCenter ArcFace、PartialFC 等路线，检测侧包含 RetinaFace 和 SCRFD。

适合它的场景：

- 追求识别准确率上限。
- 需要更强的复杂姿态、遮挡、低质量图像鲁棒性。
- 能接受更复杂的模型包、部署方式、授权检查和运行时依赖。
- 有完整的 A/B 评测和阈值校准流程。

对本项目的限制：

- InsightFace 主生态偏 Python/C++，直接进入 Rust 主线会增加集成复杂度。
- 部分预训练模型和模型包存在非商业或需联系授权的限制，不能只看代码 MIT 许可证。
- 更强模型不等于登录体验一定更好；摄像头启动、模型加载、推理延迟、失败恢复同样重要。
- 若切换识别模型，旧 embedding 模板不能复用，必须要求重新注册。

当前判断：InsightFace 系列应作为第二阶段候选，不应在没有本项目 calibration report 的情况下直接替换当前主线。

### OpenCV YuNet + SFace

OpenCV Zoo 官方提供 YuNet 检测和 SFace 识别模型。YuNet 是轻量人脸检测模型，OpenCV 文档说明其 WIDER Face 验证集指标为 easy/medium/hard 三档 AP。SFace 是 MobileFaceNet 实例配合 SFace loss 的人脸识别模型，OpenCV Zoo 给出了精度评估和 C++/Python 示例。

适合它的场景：

- 本地、离线、轻量、CPU 运行。
- Windows 桌面部署。
- 需要尽快建立稳定 baseline。
- 登录链路中不希望引入过重模型和额外运行时。

风险：

- 识别上限可能低于 ArcFace/InsightFace 系列。
- 复杂姿态和低质量摄像头下效果需要实测。
- 当前 OpenCV Zoo 里的 2026 YuNet 主要解决动态输入形状和 OpenCV 5 ONNX Runtime engine 兼容，不代表识别效果比 2023 版本天然更强。

当前判断：保留为 Phase 5.5 baseline，先把可视化、采样、阈值校准做完整。

### MediaPipe Face Landmarker

MediaPipe Face Landmarker 输出 3D 人脸地标、表情 blendshape 和面部转换矩阵，适合做人脸关键点可视化、姿态估计、眨眼/表情辅助和 AR 效果。它不是专门用于身份识别的 embedding 模型。

适合它的场景：

- 显示模型“看到了什么”。
- 估计头部姿态，标注左转、右转、低头、抬头。
- 做活体辅助特征，例如眨眼、表情变化、头部轻微运动。
- 辅助过滤低质量注册帧。

不适合它的场景：

- 直接替代 ArcFace/SFace 作为身份识别主模型。
- 单靠 landmark 坐标做安全认证。

当前判断：可以作为 Phase 5.5 的姿态和可视化增强候选，但不替代当前 SFace 身份识别。

## 关键技术问题

### shape 是什么

这里的 shape 指模型输入张量形状，通常包括 batch、channel、高度、宽度。例如人脸检测模型可能要求输入图像缩放到固定高宽，也可能支持动态高宽。

静态 shape：

- 模型输入尺寸固定。
- 推理引擎优化更容易，性能更可预测。
- 使用时通常需要把图像缩放到指定尺寸。

动态 shape：

- 模型可以接受不同高宽。
- 灵活性更好，适合不同摄像头分辨率。
- 某些推理后端初始化和优化更复杂，性能不一定更好。

对本项目的判断：

- 当前使用 OpenCV 4 路线时，优先稳定，不需要为了“新”而切 2026 动态 shape 模型。
- 如果后续切 OpenCV 5 ONNX Runtime engine，再评估 2026 YuNet 动态 shape。
- 动态 shape 解决的是输入尺寸兼容问题，不是身份识别准确率问题。

### int8 / int8bq 是什么

int8 是普通 8-bit 量化模型，目标是降低模型体积和推理计算量。代价通常是少量精度损失。

int8bq 是 block-wise quantized，按块量化。它试图在压缩和精度之间取得更好平衡。OpenCV Zoo 文档中 YuNet/SFace 的 block quantized 版本评估结果与原模型非常接近。

对本项目的判断：

- 当前不优先使用量化版本，先用原始 FP32/默认模型建立准确 baseline。
- 只有当性能、启动时间或包体成为问题时，再用同一套 calibration report 对比 int8/int8bq。
- 不能仅因为量化版本指标接近就直接替换登录链路模型。

### 单张注册照还是多张注册照

单张正脸照通常会得到更稳定、更高的正脸匹配分数，但覆盖不了侧脸、低头、背光等真实场景。

多张注册照如果直接粗暴平均，可能把低质量侧脸、模糊帧、背光帧混进模板，导致正脸分数反而下降。

推荐策略：

- 注册时采集多帧，但先做质量过滤。
- 保存多个高质量模板或按姿态分组的模板，不把所有样本盲目压成一个平均向量。
- 每个模板记录采集条件、模型版本、检测质量、对齐质量和姿态标签。
- 认证时取最高匹配分，同时要求连续帧通过，避免单帧偶然误判。

### 阈值为什么不能照搬 0.85

不同模型的 score 定义不同。有的模型输出 cosine similarity，有的使用 cosine distance，有的经过归一化或模型内部标定。`0.85` 在一个系统里合理，不代表在另一个系统里合理。

本项目应该按以下指标选阈值：

- FAR：错误接受率。
- FRR：错误拒绝率。
- ROC/DET 曲线。
- EER：FAR 和 FRR 接近时的平衡点。
- 用户真实使用时的连续 N 帧通过概率。
- 登录链路可接受的等待时间。

当前建议：

- 临时阈值可以先在 `0.50 ~ 0.60` 区间试验。
- 正式阈值必须由 `face-calibrate` 输出推荐区间。
- 阈值需要绑定模型版本和模板版本，不能跨模型复用。

## CPU / GPU 路线判断

当前机器有 RTX 3050 4GB 显存。从算力上讲，YuNet/SFace 和轻量 ArcFace 类模型都不是 4GB 显存跑不动的问题。

但登录链路里，GPU 不一定更优：

- GPU 推理需要额外运行时、驱动和初始化成本。
- 锁屏/开机未登录阶段，GPU 可用性和设备选择比桌面环境更复杂。
- 小模型 CPU 推理已经足够快时，GPU 可能得不偿失。
- ONNX Runtime DirectML 仍可用，但官方说明新功能开发方向已经转向 WinML。

当前判断：

1. Phase 5.5 继续 CPU baseline。
2. 只在 CPU 延迟或功耗成为实测瓶颈时，再评估 ONNX Runtime + WinML/DirectML。
3. 如果接入 GPU，必须保留 CPU fallback。
4. Provider/LogonUI 链路不能直接依赖 GPU 初始化成功。

## 对 WinFaceUnlock 的推荐路线

### 短期

先实现 Face Auth 调试和校准能力：

- `face-debug-snapshot`：保存原图、检测框、关键点、对齐 crop、匹配分、失败原因。
- `face-calibrate`：按场景采集多帧，输出分数分布、检测成功率、耗时、推荐阈值。
- `face-scenario-sweep`：覆盖正脸、左右偏头、低头、抬头、背光、低光、眼镜反光、USB 低分辨率摄像头。

这一步可以在本机做，不碰 Credential Provider，不影响 PIN/密码保底。

### 中期

建立模型 A/B 测试框架：

- detector 和 recognizer 都从配置或 CLI 显式选择。
- 模型 manifest 记录模型族、版本、hash、输入 shape、score 语义和推荐阈值来源。
- 模板记录 recognizer identity，模型不匹配时拒绝使用旧模板。
- 同一套场景数据可重复跑 YuNet/SFace、YuNet/ArcFace、RetinaFace/ArcFace、SCRFD/ArcFace。

### 后期

如果数据证明当前路线不够，再切换模型：

- 检测不稳：优先评估 SCRFD 或 RetinaFace。
- 识别不稳：优先评估 ArcFace/InsightFace ONNX 模型。
- 姿态不可解释：引入 MediaPipe Face Landmarker 或轻量 head pose estimator。
- 性能瓶颈：评估量化模型、ONNX Runtime、WinML/DirectML。

## 决策门槛

进入模型替换评估的触发条件：

- 正脸正常光照下同一用户分数仍明显不稳定。
- 侧脸 15 到 30 度以内频繁低于阈值。
- 低像素 USB 摄像头下检测框或关键点明显抖动。
- 眼镜反光或背光导致连续误拒。
- 连续帧策略需要等待过久，影响锁屏进入 PIN/密码界面。
- 当前模型 A/B 报告显示替代模型在同等延迟下显著降低 FRR，且不提高 FAR。

未满足这些条件前，不为了“模型更有名”替换主线。

## 来源

- OpenCV Zoo YuNet README：https://github.com/opencv/opencv_zoo/blob/main/models/face_detection_yunet/README.md
- OpenCV Zoo SFace README：https://github.com/opencv/opencv_zoo/blob/main/models/face_recognition_sface/README.md
- InsightFace README：https://github.com/deepinsight/insightface
- MediaPipe Face Landmarker：https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker
- ONNX Runtime DirectML Execution Provider：https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html

