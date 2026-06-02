# Windows 原生 DLL 运行时冲突复盘

## 现象

在新增人脸位姿模块后，部分 Rust 测试二进制和 `win_service.exe` 在进入 `main` 或测试函数之前直接退出：

```text
STATUS_HEAP_CORRUPTION
0xc0000374
```

即使只执行测试列表也会失败：

```powershell
cargo test -p win_service -- --list
```

## 根因

项目同时使用 OpenCV 和 SQLCipher 的 vcpkg 原生 DLL。

`video_provider/build.rs` 会从：

```text
vcpkg_installed/x64-windows/bin
```

复制 release runtime DLL。

但 `credential_store/build.rs` 原先在 Rust debug profile 下从：

```text
vcpkg_installed/x64-windows/debug/bin
```

复制 debug runtime DLL，同时链接的 import library 仍然来自：

```text
vcpkg_installed/x64-windows/lib
```

也就是链接 release import library，却复制 debug runtime。两个 build script 还会向同一个 target 目录复制同名 DLL，构建顺序变化时会互相覆盖。最终进程同时加载不匹配的 OpenCV / SQLCipher 依赖，触发启动阶段 heap corruption。

## 修复

`credential_store/build.rs` 已统一复制：

```text
vcpkg_installed/x64-windows/bin
```

这和当前链接的 release import library 保持一致。

## 验证

修复后以下命令均通过：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p diagnostics_cli --features mediapipe-pose
.\target\x86_64-pc-windows-msvc\debug\diagnostics_cli.exe --help
```

## 后续约束

1. 同一 Rust profile 内，vcpkg import library 和 runtime DLL 必须来自同一 triplet 层级。
2. 不允许多个 crate 向同一 target 目录复制不同配置的同名 DLL。
3. 遇到进程在 `main` 前崩溃时，优先检查原生 DLL 搜索路径、复制来源和 debug/release 混用。
