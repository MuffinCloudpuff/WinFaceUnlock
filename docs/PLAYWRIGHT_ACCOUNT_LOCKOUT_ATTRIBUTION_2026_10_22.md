# Playwright 浏览器启动来源与账户锁定静态调查

调查日期：2026-10-22。下文现场时间均为北京时间（UTC+08:00）。

## 结论与证据强度

启动来源已定位到 **Codex 聊天“检查项目当前进展”**，任务 ID 为
`01a09381-3d7a-7fd1-9b1b-88eb3d24384a`，工作目录为
`D:\study\workspace\Rust_workspace\gd3-daemon`。该任务当时在验证 Nexus Downloader
浏览器扩展的下载进度面板，反复运行 `node tests/progress_window_smoke.cjs`。

命令记录、脚本的实际启动代码、测试目录时间，以及 Windows 的失败认证记录，构成了逐次
对应的证据链。它把此前仅知道“来自 Playwright Chromium”的结论推进到了具体聊天和脚本。
这次自动化测试影响到了真实 Windows 账户；使用独立浏览器资料目录并不隔离操作系统账户。

内部机制有同版本源码与本机 DLL 静态证据支持：浏览器检测设备认证能力时，可以使用当前
Windows 用户名和空密码调用 `LogonUserW`。测试不断创建新资料，导致检查缓存不能跨运行复用；
10 次错误密码认证达到本机阈值，随后出现账户锁定事件。

证据边界：没有事发时的调用栈、API 参数跟踪或匹配的 4688 父子进程记录。启动来源由
任务命令、文件时间与代码联合重建；内部具体触发分支由静态代码分析解释，尚未在 VM 动态验证。
不将 4625 本身当作“密码为空”的证明，不断言密码被窃取，也不把其他活跃聊天视为责任来源。

## 启动链

```text
Codex：“检查项目当前进展”
  → PowerShell 执行 node tests/progress_window_smoke.cjs
  → Node child_process.spawn(指定的 chrome.exe, 手写启动参数)
  → 新资料目录 output/playwright/progress-window/<时间戳>/profile
  → Playwright chromium.connectOverCDP(该浏览器)
  → 浏览器内部 Windows 认证能力检查
  → 错误密码事件 4625；第 10 次对应锁定事件 4740
```

任务命令显式设置的浏览器路径，与 Windows 4625 完全一致：

`C:\Users\Liu\AppData\Local\ms-playwright\chromium-1234\chrome-win64\chrome.exe`

本机文件产品名为 `Google Chrome for Testing`，版本 `151.0.7922.34`；本地 Playwright
`browsers.json` 也将 revision 1234 对应到这个版本。Node 测试读取的是下载器与本地服务的
配对令牌；该令牌用于下载器 API，不能与 Windows 账户密码混淆。本次调查未读取令牌值。

## 十次对应关系

“命令发出”是任务记录时间，“测试目录创建”是实际文件系统元数据；两者都不是 Chrome
的 OS 进程创建时间。失败 PID 来自 Windows 4625，不是根据目录推算。

| 命令发出 | 测试目录创建 | Windows 错误密码 | Chrome PID | 4625 RecordId |
| --- | --- | --- | --- | --- |
| 21:19:45.786 | 21:19:51.014 | 21:19:53.577 | 0xb6b8 | 771193 |
| 21:21:34.831 | 21:21:37.015 | 21:21:37.478 | 0xf124 | 771473 |
| 21:23:08.943 | 21:23:12.213 | 21:23:12.602 | 0xf0e0 | 771679 |
| 21:29:59.056 | 21:30:08.747 | 21:30:17.921 | 0xfa00 | 772756 |
| 21:31:28.929 | 21:31:33.881 | 21:31:35.295 | 0xf938 | 772913 |
| 21:36:44.447 | 21:36:47.648 | 21:36:48.404 | 0xcefc | 773809 |
| 21:38:22.286 | 21:38:25.452 | 21:38:26.235 | 0xf7d8 | 774237 |
| 21:42:01.065 | 21:42:05.371 | 21:42:06.618 | 0xc1d8 | 774965 |
| 21:42:32.394 | 21:42:41.670 | 21:42:42.100 | 0xc914 | 775074 |
| 21:43:53.081 | 21:43:55.321 | 21:43:55.865 | 0xfef0 | 775433 |

21:43:55.851 的 4740 / RecordId 775432 与最后一次失败属于同一次认证附近的记录；
记录顺序中锁定事件略先于 4625，不能简单写成精确毫秒上“先 4625 再 4740”。
全部失败目标是 `G15\Liu`，状态 `0xC000006D / 0xC000006A`，登录类型 2，Negotiate，本地来源。
每两次错误之间都不足策略规定的 10 分钟重置间隔，因此虽然首尾约 24 分钟，仍可累积到 10 次。

命令记录保存在：
`C:\Users\Liu\.codex\sessions\2026\09\12\rollout-2026-09-12T10-45-15-01a09381-3d7a-7fd1-9b1b-88eb3d24384a.jsonl`。
路径日期是聊天创建日；本文所用条目的时间戳是 2026-10-22。
提取后的对应表包含原始 call_id、行号与目录名，见 `target/lockout-investigation/browser-correlation.json`。

## 为什么启动浏览器会触碰 Windows 密码

以下分析使用官方 **151.0.7922.34** 标签源码，不以其他版本的上游问题报告代替本机分析。

1. **测试绕过了正常启动参数。** 脚本使用 Node `spawn` 手动启动 Chrome，再通过 CDP 接入。
   它只传入首次启动、调试端口、资料目录、扩展等参数，未包含 `--disable-field-trial-config`。
   本地 Playwright `lib/coreBundle.js:34640` 中，正常 Chromium 启动参数包含这一项；
   `connectOverCDP` 连接已存在的浏览器，不能补改启动参数。事发时的历史补丁同样没有该开关。

2. **该版本的功能实验配置启用了认证检查相关功能。** `AutofillAiWalletPrivatePasses`
   在 `autofill_features.cc:347` 的基础默认值确实是关闭；但
   `fieldtrial_testing_config.json:2707` 的 Windows 实验组会将它启用。
   `chrome_build.gni` 说明 CfT 与 `is_chrome_branded` 互斥，不能因为产品名含 Google 就当作
   普通正式版的构建分支。`variations_field_trial_creator.cc:142` 中，非普通 Chrome 品牌构建
   在未传禁用参数时采用测试实验配置；service/BUILD.gn 的实验配置编译默认开启。
   本机 DLL 中也存在实验配置应用代码的字符串。未保留运行时激活实验清单，因此这一分支
   属于静态解释，不能声称现场实际开关状态已抓取。

3. **初始化触发设备认证能力查询。** `ChromeAutofillClient` 构造函数在该功能启用时调用
   `SupportsDeviceReauth()`，随后进入 `DeviceAuthenticatorWin`。如果缓存的生物识别能力
   为 false，会调用 `AuthenticatorWin::CanAuthenticateWithScreenLock()`。生物识别能力
   是异步查询，新资料中的查询结果可能尚未写入。

4. **能力查询通过空密码尝试实现。** `password_manager_util_win.cc:219` 的
   `CheckBlankPasswordWithPrefs` 在没有有效检查缓存时调用：

   `LogonUser(username, L".", L"", LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, &handle)`。

   这里的 `L""` 是空密码。用户真实密码非空时，这种探测会成为一次错误密码认证。
   现场电脑为 WORKGROUP、`PartOfDomain=false`，符合非域账户分支的条件。

5. **新资料使检查反复发生。** 成功完成检查流程后，浏览器将结果和密码最后修改时间写到
   Local State。测试每轮新建 profile、结束后删除 profile，因此下一轮没有这个缓存。
   浏览器资料被隔离，但所有进程仍使用同一个 Windows 用户，失败计数也属于同一个用户。

同版本静态调用链为：

```text
ChromeAutofillClient 构造
  → SupportsDeviceReauth
  → DeviceAuthenticatorWin::CanAuthenticateWithBiometricOrScreenLock
  → AuthenticatorWin::CanAuthenticateWithScreenLock
  → password_manager_util_win::CanAuthenticateWithScreenLock
  → DeviceAuthenticationPresent
  → CheckBlankPassword / CheckBlankPasswordWithPrefs
  → LogonUserW(当前用户, 本机域, 空密码, 类型 2, 默认提供者)
```

## 本机 DLL 的独立静态核验

没有加载或执行 chrome.dll；使用 PE 导入表扫描与 dumpbin 静态反汇编：

- LogonUserW 延迟导入 IAT 的首选映像地址：`0x191346E68`。
- 找到的直接调用点：`0x18227802C`，位于函数范围 `0x182277DF0–0x182278169`。
- 调用前 RDX 指向 UTF-16 `.`；R8 指向 UTF-16 空字符串；R9D 为 2；第五参数为 0。
- 同一函数引用 `password_manager.os_password_blank` 和
  `password_manager.os_password_last_changed`，与源码检查缓存吻合。
- 这些是文件中的首选映像地址，不是事发时经过 ASLR 的进程地址。
- 导入引用扫描不能排除动态解析、其他模块或其他认证 API，因此不能替代运行时调用栈。

文件 SHA-256：

- chrome.exe：`409805A16D6416087E6B2F778DF1CF8F7BBB267D6B99F6B5BB0A618EACE234F2`
- chrome.dll：`026E2AD7ADDA3FAEDC022A628255207CCC6B96128B850A29803D56020272E85A`

静态核验材料保存在 `target/lockout-investigation/`，包括同版本源码快照、反汇编、脚本快照、
历史启动参数和哈希。该目录是本地调查产物，不应将其中的系统/会话记录提交或公开上传。

## 本次操作与后续边界

本次只读取现有事件、任务记录、脚本和进程，下载公开源码并做静态分析。未重跑浏览器测试、
未输入/试验 Windows 密码、未更改账户锁定策略、未修改浏览器安装或 Nexus 源码、未向另一个聊天发消息。
事发测试 profile 已由原脚本清理，无法再从它们读取当时的 Local State 或实验状态。

WinFaceUnlock 仍保持回退后的 **Stopped / Disabled**，没有安装新的人脸组件。
人脸 Provider 在账户已锁定后重试的独立缺陷，仍按原报告等待 VM 验证。

后续修复应覆盖这个浏览器测试入口：暂停在真实主账户上重跑；保留 Playwright 正常启动默认参数，
或为必须手动启动的入口显式禁用实验配置及相关认证触发功能，并防止后续参数覆盖。不能只改为
复用 profile 或提高账户锁定阈值；两者都没有消除误认证。任何开关方案仍需在有独立测试账户的
Windows VM 中验证每次启动不再产生 4625，再恢复本机测试。本次没有将静态推导宣称为已验证修复。

## 官方源码

- [Chromium 151 空密码检查](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.34/chrome/browser/password_manager/password_manager_util_win.cc)
- [Chromium 151 自动填充初始化](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.34/chrome/browser/ui/autofill/chrome_autofill_client.cc)
- [Chromium 151 功能实验配置](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.34/testing/variations/fieldtrial_testing_config.json)
- [Chromium 151 实验配置应用规则](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.34/components/variations/service/variations_field_trial_creator.cc)
- [Chromium 151 CfT 构建定义](https://chromium.googlesource.com/chromium/src/+/refs/tags/151.0.7922.34/build/config/chrome_build.gni)
- [Microsoft 4625 字段说明](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/auditing/event-4625)
