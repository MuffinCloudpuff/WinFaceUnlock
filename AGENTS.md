# AGENTS.md

## Communication

- 默认使用简体中文沟通。
- 开发前把可扩展性、健壮性和可维护性作为基础约束。
- 默认按模块化边界推进，不把多种职责长期混写在同一个模块里。
- 写代码时必须按功能和职责拆分模块；安全存储、IPC、协议、文件格式、平台 API 适配、业务策略、测试辅助等不应长期堆在同一个文件里。
- 每个功能模块都应尽量能单独测试；新增核心能力时优先给模块自身补单元测试，再通过上层集成测试串联。
- 平台相关或 unsafe 代码必须隔离在明确的 adapter/bindings 模块中，不要泄漏到业务策略和上层编排代码。
- 每个功能模块都要追求该模块目标下的效果最优，而不是为了最快推进选择临时、粗糙或后续必然推翻的路线；技术选型必须服务于该模块的长期可用性、易集成性、可靠性和可维护性。
- 不为了“先做个最小原型”牺牲模块的正确边界、接口质量、数据模型和未来集成路径；如果确实采用临时实现，必须明确标记临时范围、替换条件和清理计划。
- 接口、协议、状态、事件、错误和布尔含义的命名必须语义清晰，体现所属层级和判定对象；禁止用含糊的 `success`、`ok`、`flag`、`true/false` 在多层流程中传递关键语义。
- 当存在多层成功/失败判定时，必须命名为具体语义，例如 `auth_match_passed`、`grant_issued`、`credential_decryption_succeeded`、`pipe_delivery_confirmed`，避免不同层的“成功”混淆。
- 协议字段和公共接口一旦可能跨模块、跨进程或持久化，应优先使用枚举、结构化状态和明确错误类型，不用裸 bool 或字符串拼接协议表达复杂状态。
- 遇到问题先追踪根因，不用表面补丁掩盖结构性问题。
- 新路线确认后围绕新路线收敛实现；旧路线废弃后优先移除，短期保留时必须标记为临时过渡。

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

## GitNexus CLI

当前环境默认不使用 GitNexus MCP 工具。所有 GitNexus 操作默认使用 CLI：

```powershell
npx gitnexus status
npx gitnexus analyze --skip-agents-md
npx gitnexus analyze --force --skip-agents-md
npx gitnexus analyze --embeddings --skip-agents-md
npx gitnexus clean --force
npx gitnexus list
```

如果没有显式可用的 GitNexus MCP 工具，不要继续尝试 MCP 调用；直接用 CLI、本地搜索、编译器和测试做影响分析。

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
