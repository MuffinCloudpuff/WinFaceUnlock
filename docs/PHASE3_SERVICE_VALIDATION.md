# Phase 3 Windows Service 验证说明

更新时间：2026-05-31

## 当前范围

Phase 3 的目标是验证后台 Service 主链路，不接 Credential Provider，不接真实摄像头。

已实现内容：

1. `win_service --console-smoke`：进程内验证 `HealthCheck -> WakeAuth -> FetchCredential`。
2. `win_service --pipe-once --pipe-requests N`：控制台 named pipe host。
3. `win_service --service`：Windows Service Control Manager 入口。
4. `installer_cli install-service`：注册 `WinFaceUnlockService`，以 LocalSystem 运行，自动启动。
5. `installer_cli start-service / stop-service / service-status / repair-service / uninstall-service`。
6. 服务安装时配置延迟自启动和有限失败重启策略。

## 普通权限验证

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo run -p win_service -- --console-smoke
cargo run -p installer_cli -- install-service --dry-run
cargo run -p installer_cli -- repair-service --dry-run
```

## 管理员权限验证

以下命令必须在管理员 PowerShell 中运行：

```powershell
cargo build -p win_service -p installer_cli -p diagnostics_cli
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe install-service --start
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe service-status
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe health-check
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe wake-auth --session-id phase3-admin-test
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe fetch-credential --session-id phase3-admin-test --grant-id dev-grant-1 --nonce dev-nonce-1
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe stop-service
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe uninstall-service
```

预期：

1. `service-status` 显示 `Running`。
2. `health-check` 返回 `HealthOk`。
3. `wake-auth` 返回 `AuthSucceeded`，并输出 `grant_id`、`nonce`、`session_id`。
4. `fetch-credential` 返回 `CredentialReady`，只包含 `credential_ref`，不输出明文密码。
5. `uninstall-service` 后服务从 SCM 删除。

## 注意事项

当前 Phase 3 使用模拟认证和开发凭据引用，只验证 Service / Credential Store / IPC / grant 链路。真实人脸识别、人脸模板注册和 Credential Provider 登录界面集成在后续阶段完成。
