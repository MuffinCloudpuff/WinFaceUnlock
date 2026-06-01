# WinFaceUnlock Agent Notes

本项目默认走 Rust 后端主线，先跑通 Credential Store、IPC、Windows Service、Face Engine，再进入 Credential Provider 和 UI。

代码必须按功能职责拆分：安全存储、IPC、协议、文件格式、平台 API 适配、业务策略、测试辅助等不要长期堆在同一个文件。每个核心模块应尽量能单独测试；平台相关或 unsafe 代码隔离在 adapter/bindings 模块。

每个功能模块都要追求该模块目标下的效果最优，不能为了最快原型牺牲长期可用性、易集成性、可靠性和可维护性。接口、协议、状态、事件、错误和布尔含义的命名必须表达具体语义层级，避免用 `success`、`ok`、`flag` 或裸 `true/false` 表达跨层关键含义；跨模块/跨进程/持久化 contract 优先使用枚举、结构化状态和明确错误类型。

GitNexus 默认使用 CLI，不调用 MCP：

```powershell
npx gitnexus status
npx gitnexus analyze --skip-agents-md
```

修改既有符号前用本地方式做影响分析：

```powershell
rg "SymbolName" .
```

收尾检查：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
git status --short
```
