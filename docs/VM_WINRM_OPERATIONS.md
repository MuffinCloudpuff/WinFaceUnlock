# WinFaceUnlock VM WinRM Operations

本项目默认通过 WinRM 命令行操作 Windows VM，不通过 VMware 窗口点击。

## 固定入口

- VM 地址：`192.168.204.129`
- 凭据文件：`$env:USERPROFILE\.winfaceunlock-vm-cred.xml`
- VM 安装目录：`C:\WinFaceUnlock`
- 服务名：`WinFaceUnlockService`
- 服务进程：`win_service.exe`
- 诊断工具：`C:\WinFaceUnlock\diagnostics_cli.exe`

全局 Codex skill：

```text
C:\Users\Liu\.codex\skills\winfaceunlock-vm
```

后续只要是查看 VM 服务状态、资源占用、远程触发 diagnostics，都应先使用该 skill。

## 快速命令

在仓库根目录或任意 PowerShell 中运行：

```powershell
powershell -ExecutionPolicy Bypass -File C:\Users\Liu\.codex\skills\winfaceunlock-vm\scripts\invoke-winfaceunlock-vm.ps1 -Mode status
powershell -ExecutionPolicy Bypass -File C:\Users\Liu\.codex\skills\winfaceunlock-vm\scripts\invoke-winfaceunlock-vm.ps1 -Mode idle
powershell -ExecutionPolicy Bypass -File C:\Users\Liu\.codex\skills\winfaceunlock-vm\scripts\invoke-winfaceunlock-vm.ps1 -Mode auth
powershell -ExecutionPolicy Bypass -File C:\Users\Liu\.codex\skills\winfaceunlock-vm\scripts\invoke-winfaceunlock-vm.ps1 -Mode restart
powershell -ExecutionPolicy Bypass -File C:\Users\Liu\.codex\skills\winfaceunlock-vm\scripts\invoke-winfaceunlock-vm.ps1 -Mode modules
```

`-Mode all` 会依次执行状态检查、空闲资源采样和一次 `service-camera-auth` 认证触发采样。
`-Mode restart` 用于重启 `WinFaceUnlockService` 后观察冷启动基线。
`-Mode modules` 用于列出 `win_service.exe` 已加载模块和模块映射体积。

## 手工流程

```powershell
Test-WSMan 192.168.204.129
Set-Item WSMan:\localhost\Client\TrustedHosts -Value "192.168.204.129" -Concatenate -Force
$cred = Import-Clixml "$env:USERPROFILE\.winfaceunlock-vm-cred.xml"

Invoke-Command -ComputerName 192.168.204.129 -Credential $cred -ScriptBlock {
  Get-Service WinFaceUnlockService
  Get-Process win_service -ErrorAction SilentlyContinue
}
```

## 资源占用口径

- 常驻占用看 `win_service.exe` 的 `WorkingSet64` 和 `PrivateMemorySize64`。
- 空闲 CPU 用进程累计 CPU 秒差计算，不直接看任务管理器瞬时抖动。
- 认证瞬时占用需要一边触发 `diagnostics_cli.exe service-camera-auth`，一边采样 `win_service.exe`。
- `windows_provider.dll` 是 Credential Provider DLL，由 `LogonUI.exe` 加载，不会单独显示为进程。
- `NoFaceDetected` 表示认证路径被唤醒但没有检测到脸；可用于观察摄像头/识别链路启动时的资源变化，但不能代表成功登录路径。

## 最近一次基线

2026-06-02 在 VM 中观测到：

- `WinFaceUnlockService`：`Running`
- 启动类型：`Automatic`
- `win_service.exe` 空闲 5 秒平均 CPU：`0%`
- 空闲 Working Set：约 `125 MB`
- 空闲 Private Memory：约 `110 MB`
- 触发一次 `service-camera-auth` 且结果为 `NoFaceDetected` 时：
  - Working Set 峰值：约 `151 MB`
  - Private Memory 峰值：约 `136 MB`
  - 结束后回落到约 `127 MB` / `111 MB`

这些数字是参考基线，不是硬性性能预算。
