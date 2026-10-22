# 长期运行、启动与摄像头可靠性排查（2026-10-22）

本次基于当前工作区重建整个项目的代码图谱，并重点追踪 Rust 主线的
安装器 → Windows Service → IPC → 人脸认证 → 摄像头适配器，以及凭据和模板存储。
保留了排查开始前已有的未提交修改。历史参考项目也被索引，但本次修复针对当前
Rust workspace；不是对仓库每一行代码或所有历史版本的安全审计。

没有在已检查的模板读取、匹配和存储路径中发现“使用两三个月后模板自动过期”的规则。
以下问题可以造成类似症状，但没有受影响电脑的版本、日志和设备信息，尚不能确认
它们就是现场故障的唯一原因。

## 已修复的问题

| 优先级 | 问题与触发条件 | 修改后行为 | 位置 |
| --- | --- | --- | --- |
| P1 | 客户端在服务端 `ConnectNamedPipe` 前退出，连接失败路径没有断开旧实例，后续请求可能持续失败 | `receive` 对连接失败也重置管道；增加确定性断连时序验证 | `crates/ipc/src/named_pipe.rs` |
| P1 | 设备枚举仅释放指针数组，未释放每个 `IMFActivate`；后台每 30 秒枚举会持续累积 COM 引用 | 逐个接管并释放接口，再释放数组 | `crates/video_provider/src/mf_bindings.rs` |
| P1 | 自动恢复序列最后为 `None`，第三次及之后故障不再重启 | 5 秒、30 秒、随后持续 120 秒退避重启；正常停止不触发恢复 | `crates/installer_cli/src/service_manager.rs` |
| P1 | 管道宿主初始化等错误最终仍上报成功退出，SCM 无法按非崩溃故障恢复 | 记录错误，并上报非零服务退出码 | `crates/win_service/src/service_host.rs` |
| P1 | 摄像头后端只要 `is_opened` 成功就被选中，即使一直读不到图像 | 每个后端最多尝试读取 8 次；失败释放后尝试下一个；成功的首帧保留给调用者 | `crates/video_provider/src/capture_open.rs`、`opencv_camera.rs` |
| P2 | 管道尚不存在时 `WaitNamedPipe` 立即返回，20 次重试可能瞬间结束 | 失败等待补足每轮 100 毫秒间隔，约 2 秒尝试窗口，成功清除旧连接错误 | `crates/ipc/src/named_pipe.rs` |
| P2 | 后台探测因锁屏、租约占用或暂时故障跳过后，设备签名未变便不再尝试 | 保留待刷新请求，下一轮继续；线程创建失败复位；不因每次解锁重复执行已完成的探测 | `crates/win_service/src/camera_backend_profiles.rs` |
| P2 | 解锁认证只在配置加载时读取首选后端，后台更新后仍长期使用旧值 | 每次打开认证摄像头前重新读取后端配置 | `crates/win_service/src/auth_issuer.rs` |
| P2 | 服务日志始终追加，长期使用没有大小边界 | 当前日志 8 MiB 滚动，保留 3 个历史文件；进程内串行写入 | `crates/win_service/src/service_log.rs` |

日志限制针对 `service.log`，并非所有日志、照片、录入目录的统一配额。
升级前已经超大的历史日志可能暂时保留在备份中。

## 启动优先级调整

安装配置原本就是 `AutoStart`，并且 `set_delayed_auto_start(false)`。
本次在独立 Windows adapter `service_priority.rs` 中将服务进程调度级别设为
`AboveNormal`；设置失败记录日志，继续提供服务。该设置在服务启动时执行。

这会提高 CPU 调度优先级，不保证 SCM 在其他所有自动服务之前启动它，也不能
加速尚未就绪的摄像头驱动。此次对启动可用性更直接的修复是管道真实等待和服务恢复。
没有修改全局服务启动分组，没有使用 High 或 Realtime。

恢复策略在运行新版安装/修复流程后才会写入已安装服务；仅修改源码或替换服务
可执行文件不会更新 SCM 的故障动作。优先级调整需新版服务进程重新启动。

## 第二轮补充修复

### 入侵截图关闭必须真正停止写入

- 截图许可采用可撤销代际策略：关闭先撤销旧任务，等待已开始的写入结束，再确认关闭。
  重新开启创建新许可，旧任务不能恢复写图能力。配置重载失败也不会恢复旧许可。
- 截图开关的注册表值优先于遗留环境变量；缺失或损坏默认关闭。关闭截图不要求相机、
  人脸模板正常。UI 等服务确认后更新状态，失败不假报成功。
- 后台人体离座检测固定禁用调试图片输出，堵住不经过入侵截图策略的另一条写盘入口。
  显式运行 diagnostics 的抓图命令仍保留。
- 配置重载开始即撤销旧认证实例及缓存；加载新模板或配置失败后不继续用旧模板发放授权，
  设置与修复 IPC 仍可用。
- IPC 层在配置修改前撤销已发放授权，成功/失败都不保留旧授权；同一 grant 重复返回
  不能复活已兑换或已撤销的权限。撤销记录仅保留到到期，避免长期增长。
- 相机授权编号使用进程级序列，重载不会从头重用编号；序列耗尽时拒绝授权。

### 稳定设备绑定与扫描算法

- 保存设备接口身份 `windows-device:<device interface path>`，名称仅用于显示。
  Media Foundation / DirectShow 分别解析自己的当前索引，不再共用枚举编号或 CAP_ANY。
- 旧 `opencv-index:N` 在首次迁移时绑定当前 Media Foundation 第 N 个设备并持久化。
  旧编号本身无法证明几个月前对应的物理设备；初次升级应核对所选设备。
  已绑定设备缺失时不自动换成同名摄像头；迁移失败保持认证不可用，不能降级到测试认证。
- 枚举只读设备元数据。稳定 ID 排序后作为变化签名；成功探测有效期 24 小时，失败重试
  冷却 5 分钟。优先尝试历史可用后端，首个在 3 秒内返回有效首帧的后端即可结束探测，
  移除无条件睡眠和重复穷举。该策略寻找可用后端，不声称测出全局最快后端。
- 每个后端独立持有摄像头租约，锁屏后停止下一次探测；中断设备不刷新成功/失败时间。
  认证读取后端缓存不再带前台睡眠重试。

具体身份映射和缓存契约见 [CAMERA_BACKEND_PROFILING.md](CAMERA_BACKEND_PROFILING.md)。

### 原生摄像头调用增加进程超时边界

- 新增 Rust `camera_worker.exe`，OpenCV 打开、首帧预热、读帧在子进程执行。
  每个后端打开限时 8 秒，单次读取限时 3 秒；超时关闭 Job 并终止子进程，父进程有界回收。
- 后端失败后启用新进程尝试下一后端。一次 open 只绑定一次旧编号，切换后端期间不会
  再根据变化后的编号选另一个设备。
- Windows Job 设置 kill-on-close，服务退出后清理孤立子进程；先加入 Job，再发起摄像头请求。
  私有管道传输有界二进制帧，不写图片文件。协议损坏终止进程，普通驱动读帧错误允许重试。
- 安装包、开发启动脚本和安装前校验均纳入 `camera_worker.exe`。更新时必须一起发布，
  不能只替换 `win_service.exe`。设备元数据枚举仍使用进程内平台 API。

### 凭据与密钥持久化

- 新密钥先写同目录临时文件并同步，再以不覆盖已有文件的原子操作发布；并发初始化的
  后到者读取胜出的密钥，不能覆盖既有主密钥。
- 密钥或数据库只丢失其中一个时，直接报告存储不可用，不能生成新密钥配旧数据库。
  解密失败、关联记录损坏或读取失败不再假报为“未保存密码”。
- 固定路径另有初始化标记：首次安装的空目录可以初始化；完成初始化后即使两个文件
  都丢失也拒绝悄悄重建，避免把丢失的数据伪装成首次安装。
- 保存密码和账户关联使用同一数据库事务；禁止不同用户共享同一个凭据引用，防止
  覆盖另一用户的密文及认证关联；更新保留已有记录关系。
- 安装/修复在修改服务路径前固定原凭据目录，存放于独立的
  `HKLM\SOFTWARE\WinFaceUnlock\Storage`。升级后服务和录入统一读取固定位置，
  避免安装目录变化后误读空库。保留数据卸载保留该路径；明确删除数据时才清除。

本次增强不能找回已经丢失且没有备份的主密钥；DPAPI 解密环境损坏也需要现场处理。

## 仍需现场验证的环境因素

### 凭据、模板与环境应分别判断

- 人脸匹配通过后，系统仍使用保存的 Windows 密码凭据进行登录。用户或策略更改密码后，
  保存值可能过时；需要更新凭据，不能直接认定为模板损坏。
- 模板包含模型兼容性检查。模型升级而模板未迁移时，应检查 `TemplateModelMismatch`。
- 光照、镜头、外观变化可能影响匹配分数，需要实际样本与日志验证；本次没有放宽匹配
  阈值或活体检测规则，也没有启用无监督模板自动学习。
- 相机隐私权限、其他应用独占、驱动/USB 电源状态仍需在故障电脑上复现；单元测试无法
  证明设备恢复行为。

## 验证结果与范围

- `cargo fmt --check`：通过。
- `cargo clippy --workspace --all-targets`：通过，已有 unused import、unsafe 边界和风格警告仍存在。
- `cargo test --workspace`：475 passed，0 failed，2 ignored（含文档测试通过）。
- 控制界面 `npm run lint`、修改的 PowerShell 脚本语法解析、`git diff --check`：通过。
- 已覆盖：断连前尚未 accept 的管道恢复、延迟创建服务管道、不可读后端释放与切换、
  预热重试、读取次数上限、刷新请求保留、服务故障退出码、持续恢复配置、日志滚动。
- 第二轮新增覆盖：截图撤销与旧任务隔离、重载失败不使用旧模板、grant 撤销与防重复兑换、
  稳定身份重排/缺失/别名规则、worker 超时/协议错误/Job 回收、密钥原子发布与并发创建、
  数据库事务回滚、凭据关联保护、升级路径保持和缺 worker 的安装拒绝。
- 全工作区首次测试暴露了已有 IPC 恢复测试失败；修复并明确时序后重新运行全套通过。
- `detect_changes` 已检查调用影响；其 HEAD diff 包含本次开始前的工作区修改，不能把所有
  返回项都当成本次新增修改。
- 本机可编译运行测试。`cl`、`ninja` 不在初始 shell PATH，`cmake`、`vcpkg` 可找到；
  Cargo/MSVC 实际构建通过。
- 本轮初始代码验收时尚未安装到真机；随后按用户要求完成本机服务与控制界面升级，见下节。
  未运行 VM Credential Provider 部署验证，未完成数月 soak test。
- 显式运行本机摄像头元数据验证：C920 的 MF/DirectShow 路径别名匹配，枚举耗时约
  15 ms；未打开图像流。这个结果不能代表所有驱动、端口和设备。

最终构建日志保存在本地 `target/reliability-round2-tests.log` 与
`target/reliability-round2-clippy.log`；第一轮记录仍保留在 `target/long-running-reliability-*.log`。
这些日志不作为发布包内容。

## 本机应急回退记录（2026-10-22）

- 用户在升级后报告 Windows 提示“引用的账户当前已锁定且可能无法登录”，随后明确要求先回退。
  本机已从 `D:\tools\WinFaceUnlock-backup-20261022-214809` 恢复旧版及原设置；
  432 个旧版程序、库、凭据和人脸文件通过哈希校验。
- 为避免继续自动提交凭据，服务保持 Stopped + Disabled，WinFaceUnlock 的 Credential
  Provider 枚举入口已临时停用；Windows 自带密码/PIN 登录入口未修改。没有重启服务或主动锁屏。
- 回退前状态另存于 `D:\tools\WinFaceUnlock-before-rollback-20261022-215736`，
  回退结果见 `target/rollback-local-upgrade-result.json`。原升级记录保留作审计，不能视为当前运行版本。
- 回退时本机 WMI 查询 `Liu` 的 `Lockout=false`，不代表锁定原因已查明或已验证真实登录。
- 更正验证范围：`CheckWindowsCredential configured=true` 只证明保存的凭据可以读取/解密，
  不能证明 Windows 接受该密码。本次尚未确认锁定由旧密码重复提交、其他认证路径或其他原因造成。

## 本机升级记录（2026-10-22，随后已回退）

- 已重新构建 Release Rust 服务、摄像头 worker、Tauri 控制界面和 WinUI 安装器；
  `validate_setup_package.ps1 -SkipBuild` 解包、后端检查和临时目录落盘校验通过。
- 已升级 `D:\tools\WinFaceUnlock`，逐一核验 85 个落盘文件与 payload 一致。
  备份位于 `D:\tools\WinFaceUnlock-backup-20261022-214809`，仅 SYSTEM 和管理员可访问。
- 服务状态 Running，健康检查 HealthOk，进程优先级 AboveNormal；自动且非延迟启动，
  故障重启等待为 5/30/120 秒，非崩溃故障恢复已启用。托盘已重新启动。
- 原主密钥、人脸模板的 SHA-256 保持一致；升级前后通过仅返回状态的
  `CheckWindowsCredential` 验证已有凭据 `configured=true`，没有读取或输出密码。
- 原 `opencv-index:0` 已迁移为 C920 的 `windows-device:` 标识；新版已安装诊断程序
  可枚举该设备并读取 640×480 Bgr8 图像帧。该检查未保存图像或触发登录认证。
- 入侵截图设置保持关闭，登录唤醒模式保持原值。服务重启后处于等待会话事件状态，
  首次解锁/登录前后台相机探测延期符合当前策略；本次未主动锁屏或验证实际解锁。
- VM WinRM 不可用，因此保留原 Credential Provider DLL 及注册，未把新 Provider 部署到本机。
- 安装包：`target/setup-package/WinFaceUnlockSetup.exe`；SHA-256：
  `A86FD36A47A42E1444F3C08FE6ED6FE4D457CF2C832EB095A985A5973BEFB6AF`。
  部署结果位于 `target/reliability-local-upgrade-result.json`。
- 打包脚本同时修正 Debug 参数/后端配置传递，并移除错误的 Tauri cdylib 强制校验：
  实际控制程序通过 Rust rlib 链接，该 DLL 不是其动态运行依赖。默认 Release 行为保持不变。

## 防重试版本再次本机安装（2026-10-22 22:55）

- 在锁定来源调查后，用户重新明确要求安装到本机试用；这取代此前本机保持停用的指示。
  已向用户说明没有 VM 验证，未主动锁屏或试密码。此次同时更新服务及 Provider。
- 重新通过 `cargo fmt --check`、`logon_safety`/`windows_provider` 43 项针对性测试，
  构建 Release 服务、Provider、Tauri UI、安装包；复用未变更的 WinUI 安装界面产物。
  安装包解包、WinUI/后端连接、预检及暂存验证均通过。首次验证因调用方未创建暂存目录
  的父目录失败，补齐父目录后通过；没有修改预检逻辑或跳过检查。
- 安装位置 `D:\tools\WinFaceUnlock`；安装前完整备份及注册表导出位于
  `D:\tools\WinFaceUnlock-before-guarded-install-20261022-225458`，仅管理员与 SYSTEM 可访问。
  88 个文件与 payload SHA-256 一致。旧 Provider 文件保留，注册已切换到
  `provider\windows_provider-2f29a6654812.dll`。
- 服务 Running / Auto，非延迟启动，管理员部署脚本确认进程优先级 AboveNormal；
  安装后独立调用 `health-check` 再次返回 HealthOk，托盘已启动。
- 备份中的主密钥、数据库、人脸模板与安装前哈希一致；安装后主密钥和模板哈希保持一致，
  原凭据库固定在 `D:\tools\WinFaceUnlock\credential-store`，未重新录入、替换或输出密码。
  CameraId 已迁移为稳定的 `windows-device:` 标识；入侵截图仍为 false，原有
  `background-silent-recognition` 模式保留。其他 Windows Provider 注册值逐项比较未变更。
- 22:56 只读 WMI 查询账户 `Liu` 的 `Lockout=false`。这仅表示查询时未锁定，
  不代表已验证密码正确、真实 LogonUI 回调、VM 稳定性或长期运行稳定性。
- 新安装包 `target/setup-package/WinFaceUnlockSetup.exe`，SHA-256：
  `4EC9E580275DF602C0531BCD807B547BE348708366A6AA184157EEE6300A24AD`。
  安装报告 `target/install-guarded-local-result.json`，独立运行状态查询
  `target/guarded-install-postcheck.json`。后者在普通用户权限下无法读取服务进程优先级，
  该字段为空；AboveNormal 依据前者的管理员查询结果。
- 紧急停用登录入口：以管理员身份运行安装目录中的 `recovery\emergency-disable.cmd`。
  该入口只停用 WinFaceUnlock Provider，不删除数据，也不改动其他 Windows 登录入口。
  本次未执行这一停用命令。

## 锁屏摄像头启动延迟回归（2026-10-22，23:16 已安装）

- 用户明确反馈锁屏后摄像头启动慢。现场两次 `OpenCameraStarted` 到
  `OpenCameraSucceeded` 分别为 8746 ms（22:58:05）和 8485 ms（22:58:20）；
  成功打开后到接受人脸分别约 108 ms 和 176 ms。
- 升级前 C920 的 DirectShow 快速配置仍以 `opencv-index:0` 保存；迁移稳定 ID 后
  查不到该记录。无偏好时旧默认先试 Media Foundation，与隔离 worker 的 8 秒超时
  及随后约 0.36 秒的 DirectShow 打开吻合。现有日志没有逐后端失败记录，因此超时
  归因由代码路径及耗时共同支持，而非捕获到驱动内部调用栈。
- 服务启动时界面状态 Unknown，后台探测依法延后；首次解锁后才为稳定 ID 生成
  DirectShow 记录（22:58:50，open_ms=357）。随后的生产日志打开约 0.35–0.38 秒，
  不应把这一缓存生效效果误报为尚未部署的源代码修复效果。
- 本次在 `video_provider/backend_policy.rs` 统一前台、worker 和探测的候选顺序：
  保留实测偏好；没有偏好或遗留 Any 时先 DirectShow，再 Media Foundation。
  后者仍可作为实测首选或失败回退。稳定 ID 解析、8 秒隔离超时、首帧校验保持。
  没有用旧编号/同名记录猜测新设备身份，也没有放开锁屏后台探测。
- 服务成功打开日志新增真实后端与 `open_ms`；诊断命令新增
  `camera-open-benchmark --backend auto`，可在不保存图片、不认证 Windows 密码的情况下
  验证普通后端选择路径。
- `video_provider`、`win_service`、`diagnostics_cli` 共 157 项测试通过，1 项原有测试忽略；
  `cargo fmt --check` 和相关 all-targets Clippy 通过（仍有既有 warnings）。安装包构建、
  解包、WinUI 后端连接和暂存自检通过。相关日志前缀
  `target/lockout-investigation/camera-startup-`。
- 新安装包 SHA-256：`A728CD6D81A288646B107ED91EECBF649FE21FC82123128CAF40C291F50FA29F`。
  Provider SHA-256 与当前安装版本完全一致：
  `2F29A665481287E510469F901A655869E4B526B27FAFDD84AB7831820D191C6E`。
- 用户确认回到桌面后于 23:16 安装成功，核验 85 个文件；备份为
  `D:\tools\WinFaceUnlock-before-camera-startup-20261022-231620`。服务 Running / Auto，
  HealthOk，Provider、主密钥、人脸模板和开关保持一致。
- 23:17 独立进程、无后端配置文件的三次真实单帧测速为 1078 / 778 / 893 ms，
  已安装路径为 805 ms，均直接选择 DirectShow。测量包含 worker 启动及首帧，
  没有存图或请求 Windows 认证；不能当作整条锁屏解锁耗时。报告为
  `target/camera-startup-measurements.json`，账户查询 Lockout=false。

## 摄像头周期访问与正常释放（2026-10-22，23:29 已安装）

- 用户确认摄像头保护弹窗程序为 `camera_worker.exe`。上述四次显式测速会产生该进程；
  之后的进程快照仅见服务，没有 worker。不能据此把用户全部周期弹窗都归因于测速。
- 当前本机 `PresenceLockEnabled=false`、`IntruderSnapEnabled=false`，离座检测停用。
  已有键鼠门控保持：最近 60 秒有输入不采样；输入状态缺失/过期不采样；空闲后短采样，
  存在人员时动态延长检查间隔，缺席时复核后再锁定。锁屏人脸识别为独立业务链路。
- 发现两个实现问题：后端自动探测有独立的 30 秒循环，不经过键鼠门控；worker 的正常
  close 与超时路径一样直接终止进程。Windows 用户摄像头访问记录中的两个 worker 路径
  为 LastUsedTimeStop=0，但当时进程已不存在，所以该记录不能单独证明仍在占用。
- 已移除周期线程、解锁触发探测和读配置触发探测；显式诊断刷新在桌面状态下单次执行，
  中断后不安排后台重试。稳定设备 ID 在真实打开时仍重新解析，不依赖后台轮询。
- worker 协议加入结构化 Close/Closed；先释放驱动并确认，再正常退出。关闭确认最多等
  1 秒，退出宽限 250 ms；卡死、坏协议和父进程退出仍保留强制清理。需要配套部署父进程
  与 worker。本轮没有修改键鼠门控、离座策略、入侵截图策略或密码防重试组件。
- `video_provider`、`win_service`、`diagnostics_cli` 共 158 项测试通过，1 项原有测试忽略；
  格式检查、相关 all-targets Clippy、安装包构建和解包/暂存自检通过（Clippy 仍有既有
  warnings）。日志前缀 `target/lockout-investigation/camera-privacy-`。
- 23:29 本机更新成功，85 个文件逐项哈希匹配，服务 Running / AboveNormal / HealthOk。
  备份为 `D:\tools\WinFaceUnlock-before-camera-privacy-20261022-232946`；Provider DLL 与注册、
  主密钥和人脸模板保持。报告为 `target/camera-privacy-local-upgrade-result.json`。
  当前安装包 SHA-256：`27211D7734530228E98C43D42E46C4DD1A9A7F7B1D466CEDA8913E3355ABA7F9`。
- 23:30 提前说明后仅做一次已安装版本单帧验证：DirectShow 打开及首帧 1207 ms，
  整个诊断进程 1637 ms，结束后 worker 数量为 0；Windows 摄像头访问记录已写入晚于
  开始时间的结束时间。没有保存图片或请求 Windows 密码认证。报告为
  `target/camera-privacy-release-check.json`。该结果验证本次打开/释放，不等于完整锁屏解锁测试。
- 恢复升级前的控制界面与托盘后，连续观察 76 秒、281 次进程快照均未发现 worker；
  Windows 记录的摄像头开始/结束时间保持不变，服务 HealthOk，账户 Lockout=false。
  离座落锁与入侵截图开关仍为 false。报告 `target/camera-privacy-observation.json`。
  这是本次桌面状态下的短期观察，未模拟解锁事件或长期运行；第三方保护软件的历史弹窗
  不能仅凭这些记录逐条归因。

## 发布前与现场复核

1. 在 VM 安装或修复新版，确认自动、非延迟启动以及 `sc.exe qfailure WinFaceUnlockService`
   的最终动作仍为 Restart；确认服务日志有 `Service.PriorityConfigured class=AboveNormal`。
2. 验证连续故障恢复与人工停止：第三次及之后仍恢复，人工停止保持停止。
3. 验证锁屏/解锁、快速客户端断连、开机管道延迟创建，服务继续接受健康检查。
4. 在有真实摄像头的受控测试机做插拔、休眠恢复、多设备重排、占用后释放测试。
   记录设备列表、CameraId、后端、错误阶段及驱动版本。
5. 做持续枚举/认证压力测试，观察私有内存、句柄数和日志目录；COM 引用释放修复仍需
   以运行数据确认增长曲线。不要把“几个月使用”直接等同于“连续运行几个月”。

## Windows API 依据

- [MFEnumDeviceSources：逐个释放接口并释放数组](https://learn.microsoft.com/en-us/windows/win32/api/mfidl/nf-mfidl-mfenumdevicesources)
- [ConnectNamedPipe：旧客户端关闭后的 ERROR_NO_DATA 与 DisconnectNamedPipe](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-connectnamedpipe)
- [WaitNamedPipeW：管道实例不存在时立即返回](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-waitnamedpipew)
- [SERVICE_FAILURE_ACTIONS：最后动作会用于后续失败](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_failure_actionsa)
- [非崩溃故障恢复需要非零退出码](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_failure_actions_flag)
- [SetPriorityClass：AboveNormal 语义](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setpriorityclass)
- [Job kill-on-close：最后一个 Job 句柄关闭时终止关联进程](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
- [RegFlushKey：持久化刷新需要 KEY_QUERY_VALUE 权限](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regflushkey)
