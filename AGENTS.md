# AGENTS.md

## Communication

- 默认使用全局 AGENTS.md 的通用工程规则；本文件只补充 WinFaceUnlock 项目特定约束。
- 项目模块边界必须围绕安全存储、IPC、协议、文件格式、平台 API 适配、业务策略和测试辅助来划分。
- 平台相关或 unsafe 代码优先隔离在 adapter/bindings 边界内，不泄漏到认证策略和上层编排逻辑。
- 跨进程或持久化契约中的状态、错误和授权结果必须使用结构化类型表达，尤其避免把认证匹配、授权发放、凭据解密、管道投递混成同一个布尔成功值。

## Project Direction

WinFaceUnlock 的核心链路固定为 Rust 主线：

1. 先跑通后端链路，不先做 UI。
2. 先做 Rust workspace、Credential Store、IPC、Windows Service。
3. 再做本地摄像头和 Face Engine。
4. Credential Provider 必须在虚拟机中验证稳定后再上真机。
5. 小车视频源和 24G 雷达只作为后续增强，不进入第一阶段主链路。
6. 不使用 Python PoC、Python sidecar 或 Python 常驻服务。

## Git Boundary

- 本项目仓库根目录是当前目录：`D:\study\workspace\Rust_workspace\WinFaceUnlock`。
- 不依赖外层 `D:\study` 的 Git 仓库状态。
- 查看状态、提交、分析索引时都必须在本目录执行。

## VM Operations

- 操作 Windows VM 时默认使用全局 skill `winfaceunlock-vm`：`C:\Users\Liu\.codex\skills\winfaceunlock-vm`。
- VM 服务状态、资源占用、`diagnostics_cli` 远程触发和 WinRM 故障排查见 `docs/VM_WINRM_OPERATIONS.md`。
- 首选 WinRM 命令行：`Invoke-Command -ComputerName 192.168.204.129 -Credential $cred ...`。
- 不要默认通过 VMware 窗口点击、键盘注入或截图操作 VM；只有 WinRM 不可用且用户明确同意时才走 GUI 兜底。

### Impact Analysis Fallback

修改既有公共符号前，至少检查直接引用：

```powershell
rg "SymbolName" .
```

提交或收尾前，至少运行：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
git status --short
```

## Build Checks

当前 Codex 会话如果还没继承 Rust PATH，先临时补：

```powershell
$env:Path = 'C:\Users\Liu\.cargo\bin;' + $env:Path
```

Phase 0 验收命令：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

后续 OpenCV / SQLCipher / Windows Service 阶段前，还需要确认：

```powershell
where cl
where cmake
where ninja
where vcpkg
```
