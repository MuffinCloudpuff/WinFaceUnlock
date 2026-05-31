# WinFaceUnlock

WinFaceUnlock 是计划独立于 `PC_Client` 和小车端的 Windows 人脸解锁外挂项目。

项目必须先具备与普通本机人脸解锁工具相同的独立运行能力：

1. Windows 锁屏后，鼠标或键盘输入可以唤醒识别。
2. 默认使用本地摄像头完成识别和解锁。
3. 不依赖 `PC_Client`、小车在线状态或雷达状态才能运行。

小车端属于后续可选增强：

1. 远程视频源：提供可用于人脸识别的视频帧。
2. 人体存在触发器：通过 24G 毫米波雷达判断是否有人靠近，从而额外触发识别服务。

无论是否接入小车，最终认证、凭证有效性判断和 Windows 解锁动作都在 PC 本机完成。

## 核心边界

- 小车端不保存 Windows 凭据。
- 小车端不发放“可直接解锁”的认证凭证。
- 小车端只提供可选的视频帧、雷达存在状态和必要的健康状态。
- PC 端 FaceUnlockAgent 负责模型加载、人脸检测、人脸识别、活体/挑战校验、失败次数限制和解锁授权。
- Windows Credential Provider / UnlockBroker 作为独立本机解锁组件，不塞进 `PC_Client` 的 Python 主服务。
- `PC_Client` 只负责可选的联动配置、视频源选择和状态展示。

## 推荐主线

```text
基础独立模式：
Windows 锁屏后的鼠标或键盘输入
-> FaceUnlockAgent 唤醒
-> 从本地摄像头取帧
-> PC 端做人脸检测/识别/活体校验
-> UnlockBroker 生成短时本机授权
-> Credential Provider 执行 Windows 解锁

小车增强模式：
小车在线且 24G 雷达检测到有人靠近
-> 可额外唤醒 FaceUnlockAgent
-> 可选择从小车视频源取帧
-> 其余认证和解锁流程不变
```

## 文档

- [总体技术路线](docs/TECHNICAL_ROUTE.md)
- [详细实施方案](docs/DETAILED_IMPLEMENTATION_PLAN.md)
- [实施方案与阶段计划](docs/IMPLEMENTATION_PLAN.md)

## 参考项目

- FaceWinUnlock-Tauri: https://github.com/zs1083339604/FaceWinUnlock-Tauri

该项目对 Windows Credential Provider、命名管道、OpenCV YuNet/SFace 人脸识别链路有参考价值。但其公开源码不完整，且协议/凭据安全边界需要重新设计，本项目不直接照搬其实现。
