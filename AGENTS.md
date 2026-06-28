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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **WinFaceUnlock** (6927 symbols, 15288 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## When Debugging

1. `gitnexus_query({query: "<error or symptom>"})` — find execution flows related to the issue
2. `gitnexus_context({name: "<suspect function>"})` — see all callers, callees, and process participation
3. `READ gitnexus://repo/WinFaceUnlock/process/{processName}` — trace the full execution flow step by step
4. For regressions: `gitnexus_detect_changes({scope: "compare", base_ref: "main"})` — see what your branch changed

## When Refactoring

- **Renaming**: MUST use `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` first. Review the preview — graph edits are safe, text_search edits need manual review. Then run with `dry_run: false`.
- **Extracting/Splitting**: MUST run `gitnexus_context({name: "target"})` to see all incoming/outgoing refs, then `gitnexus_impact({target: "target", direction: "upstream"})` to find all external callers before moving code.
- After any refactor: run `gitnexus_detect_changes({scope: "all"})` to verify only expected files changed.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Tools Quick Reference

| Tool | When to use | Command |
|------|-------------|---------|
| `query` | Find code by concept | `gitnexus_query({query: "auth validation"})` |
| `context` | 360-degree view of one symbol | `gitnexus_context({name: "validateUser"})` |
| `impact` | Blast radius before editing | `gitnexus_impact({target: "X", direction: "upstream"})` |
| `detect_changes` | Pre-commit scope check | `gitnexus_detect_changes({scope: "staged"})` |
| `rename` | Safe multi-file rename | `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` |
| `cypher` | Custom graph queries | `gitnexus_cypher({query: "MATCH ..."})` |

## Impact Risk Levels

| Depth | Meaning | Action |
|-------|---------|--------|
| d=1 | WILL BREAK — direct callers/importers | MUST update these |
| d=2 | LIKELY AFFECTED — indirect deps | Should test |
| d=3 | MAY NEED TESTING — transitive | Test if critical path |

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/WinFaceUnlock/context` | Codebase overview, check index freshness |
| `gitnexus://repo/WinFaceUnlock/clusters` | All functional areas |
| `gitnexus://repo/WinFaceUnlock/processes` | All execution flows |
| `gitnexus://repo/WinFaceUnlock/process/{name}` | Step-by-step execution trace |

## Self-Check Before Finishing

Before completing any code modification task, verify:
1. `gitnexus_impact` was run for all modified symbols
2. No HIGH/CRITICAL risk warnings were ignored
3. `gitnexus_detect_changes()` confirms changes match expected scope
4. All d=1 (WILL BREAK) dependents were updated

## Keeping the Index Fresh

After committing code changes, the GitNexus index becomes stale. Re-run analyze to update it:

```bash
npx gitnexus analyze
```

If the index previously included embeddings, preserve them by adding `--embeddings`:

```bash
npx gitnexus analyze --embeddings
```

To check whether embeddings exist, inspect `.gitnexus/meta.json` — the `stats.embeddings` field shows the count (0 means no embeddings). **Running analyze without `--embeddings` will delete any previously generated embeddings.**

> Claude Code users: A PostToolUse hook handles this automatically after `git commit` and `git merge`.

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
