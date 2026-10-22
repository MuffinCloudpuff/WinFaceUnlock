# 2026-10-22 本机账户锁定调查

## 当前状态

此前本机已回退，并停用服务和 Provider。完成锁定来源调查后，用户本轮重新明确要求
“把改完的人脸识别程序装上试试”。按这项最新指示，2026-10-22 22:55 已备份并安装
配套的新版服务和带 `logon_safety` 防重试保护的 Provider；服务为 **Running / Auto**，
Provider 枚举入口已恢复，Windows 原有登录入口未更改。

用户已知原虚拟机删除，**VM 验证仍未完成，真实 Windows 登录也未验证**。本轮只做构建、
安装包自检、文件哈希、服务健康检查和只读账户状态查询，未主动锁屏或提交密码。
22:56 只读查询 `Liu` 的 `Lockout=false`。完整部署记录见
`LONG_RUNNING_RELIABILITY_REVIEW_2026_10_22.md` 的“防重试版本再次本机安装”部分。

## 现场证据

以下时间均为北京时间（UTC+08:00）。通过管理员只读方式提取了 Windows Security 事件，
同时读取回退前备份的 `provider.log`、`service.log`，没有读取保存的密码。

Windows 事件 4625 中，账户 `G15\Liu` 的下列 10 次失败均记录：

- ProcessName：`C:\Users\Liu\AppData\Local\ms-playwright\chromium-1234\chrome-win64\chrome.exe`
- LogonType：2；AuthenticationPackageName：Negotiate；源 IP：`-`。
- Status：`0xC000006D`；SubStatus：`0xC000006A`（错误密码）。

| 时间 | 调用进程 PID（事件记录的十六进制值） |
| --- | --- |
| 21:19:53.577 | 0xb6b8 |
| 21:21:37.478 | 0xf124 |
| 21:23:12.602 | 0xf0e0 |
| 21:30:17.921 | 0xfa00 |
| 21:31:35.295 | 0xf938 |
| 21:36:48.404 | 0xcefc |
| 21:38:26.235 | 0xf7d8 |
| 21:42:06.618 | 0xc1d8 |
| 21:42:42.100 | 0xc914 |
| 21:43:55.865 | 0xfef0 |

21:43:55.851 记录事件 **4740 / RecordId 775432**，账户被锁定；同一次认证相关的
4625 紧随其后（RecordId 775433）。本机策略为失败 10 次锁定、锁定持续 10 分钟、
失败计数重置间隔 10 分钟。每两次失败之间都未间隔满 10 分钟。

| 时间 | WinFaceUnlock / Windows 后续记录 |
| --- | --- |
| 21:48:09 | 本次升级开始建立备份，晚于账户锁定 |
| 21:52:09.983 | 人脸组件提交凭据 |
| 21:52:10.024 | Windows 回调 `0xC0000234`：账户已锁定 |
| 21:52:25.236 | 登录凭据界面再次 `SetSelected`，再次发起识别 |
| 21:52:35.761 | 再次提交凭据 |
| 21:52:36.010 | 再次收到 `0xC0000234` |
| 21:53:08.597–08.661 | `UnAdvise → SetUsageScenario → Advise`，又开始后台识别 |
| 21:53:18.179 | 再次提交；该次未见对应 ReportResult，不能推断其结果 |
| 21:54:30.095 | 又一次提交 |
| 21:54:30.127 | Windows Security 4624，账户 Liu 登录成功 |
| 21:54:30.139 | 人脸组件 ReportResult 主/子状态均为 0 |
| 21:57:36 起 | 应用户要求回退并暂停自动登录 |

结论：最初那次锁定对应 Chromium 的 10 次错误密码认证，发生在安装之前。
WinFaceUnlock 确实在账户已经锁定后继续提交，但不能把它误判为初始锁定的来源。
21:54 的实际成功记录也比先前 `CheckWindowsCredential configured=true` 更有意义：
后者仅证明凭据可解密，不验证 Windows 是否接受密码。

## Chromium 归因边界

后续只读调查已经把来源定位到 Codex 聊天“检查项目当前进展”中的 Nexus Downloader
测试脚本 `gd3-daemon/tests/progress_window_smoke.cjs`：该聊天的 10 次运行命令、10 个测试
目录创建时间与上述 10 次失败逐次对应。它通过 Node `spawn` 启动同一路径的浏览器，再用
Playwright `connectOverCDP` 连接；每轮都新建并删除资料目录。详见
[后续启动来源与静态根因报告](PLAYWRIGHT_ACCOUNT_LOCKOUT_ATTRIBUTION_2026_10_22.md)。

进一步取得了与本机 `151.0.7922.34` 一致的官方源码，并对本机 `chrome.dll` 做只读反汇编，
确认其 `LogonUserW` 调用点使用空字符串密码、本地域和交互式登录类型。测试手动启动时缺少
Playwright 默认的 `--disable-field-trial-config`；该版本 Windows 功能实验配置会启用
`AutofillAiWalletPrivatePasses`，其初始化存在设备认证能力检查到空密码检查的调用链。

这些新证据明显加强了内部机制的解释，但不能把静态调用链等同于事发时捕获的运行时调用栈。
当时的资料目录已由测试清理；现有 4625 不记录密码，4688 没有匹配记录，因此没有完整的
OS 父子进程审计和现场函数调用栈。本次未启动浏览器复现，未修改浏览器、Windows 锁定阈值
或账户密码，也未修改/重跑 Nexus 测试。

## 已修复的项目缺陷

原调用链为 `Advise / SetSelected → 后台识别 → GetSerialization → ReportResult`。
`ReportResult` 丢弃实际 NTSTATUS，把失败也记为 Completed；`begin_wake_request` 又允许
Completed 重新启动。序列化还采用先克隆凭据、后清除的方式，并发调用存在重复取用窗口。

- 新增 `logon_safety` 模块：业务状态与 Windows 注册表/互斥量适配分离。
  自动提交前持久化 Submitted；Windows 任意非零主/子状态均转为 Rejected。
- 通过命名互斥量串行化跨进程提交。没有结果的提交保持阻断，组件重建、服务重启、
  冷却时间到期均不会自动解除。状态读写失败或损坏也不提交密码。
- 只有显式更新已固定的生产凭据库后才解除阻断；临时测试库不会触碰机器级状态。
  更新失败不解除；更新代数防止先前任务和迟到的 ReportResult 污染新凭据状态。
- `GetSerialization` 原子取走凭据，只能提交一次；打包失败释放分配的缓冲区。
- 后台请求带本地请求编号；UnAdvise 使旧编号失效，旧任务及传输重试不能恢复过期凭据。
- COM CredentialsChanged 回调移出状态互斥锁，避免同步重入卡住状态机。
- 用户看到 Windows 拒绝说明与密码/PIN 回退提示；没有增加 LogonUser 或空密码预检查。

该保护用于自动提交，不关闭 Windows 原生密码/PIN入口。若 LogonUI 在收到结果之前退出，
保护会保守保持等待状态，需要先通过原生方式登录并重新保存凭据。这项恢复体验必须在 VM 验证。

## 已完成验证与尚缺验证

已完成防护模块/Provider 43 项回归测试、全工作区 487 项测试（0 失败、2 忽略）、
Clippy 和格式检查；最后的工作线程取消调整已重跑全部 43 项相关测试。测试只使用模拟结果、
内存 guard 及序列化记录，不调用真实 Windows 密码认证。日志在 `target/lockout-investigation/`。
测试后的只读检查确认本机服务仍停用，生产 `HKLM\SOFTWARE\WinFaceUnlock\LogonSafety`
不存在，说明这些测试没有创建生产阻断状态。

重新具备 Windows VM 后必须验证：

1. 使用 VM 快照与独立测试账户，保留可用的其他管理员登录入口；同时部署匹配的新版服务与
   Credential Provider，先验证正确凭据一次成功。仅更新服务而保留旧 Provider 不具备本次防护。
2. 对测试账户保存错误凭据后只触发一次 Windows 提交，检查一次失败后进入阻断。
   反复选择、重新加载 LogonUI、重启服务和 VM 均不得追加自动提交。
3. 将测试账户置于锁定状态，确认 0xC0000234 后同样阻断；该过程不得使用真实主账户。
4. 验证没有 ReportResult 的进程退出、注册表读写失败、损坏状态都不会重试密码。
5. 通过原生登录更新保存凭据，验证阻断解除；旧工作线程和旧回调不能影响更新后的状态。
6. 验证 LogonUI 的真实回调顺序、跨会话命名互斥量、注册表 ACL/持久化，以及原生密码/PIN回退。

上述 VM 项目尚未执行，因此不能把单元测试通过当作可部署结论。

## 保留证据与参考

- `target/lockout-investigation/security-events-expanded.json`：20:00 起的失败/锁定/显式凭据事件。
- `target/lockout-investigation/security-events.json`：初次提取，包含 21:54 成功登录事件。
- `target/lockout-investigation/provider.log`、`service.log`：回退前日志尾部；完整原始日志仍在受保护备份中。
- [Microsoft：事件 4625 与错误码含义](https://learn.microsoft.com/en-us/windows/security/threat-protection/auditing/event-4625)
- [Microsoft：Credential Provider ReportResult](https://learn.microsoft.com/en-us/windows/win32/api/credentialprovider/nf-credentialprovider-icredentialprovidercredential-reportresult)
- [Chromium 相似问题](https://issues.chromium.org/issues/562403601)
- [WebView2 上游相似问题与讨论](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5722)
