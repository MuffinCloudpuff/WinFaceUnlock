# Phase 8 Graphical Installer and Setup Wizard

## Goal

Phase 8 turns the verified command-line installation flow into a normal Windows
software experience:

```text
WinFaceUnlockSetup.exe
-> UAC elevation
-> choose installation directory
-> copy binaries and models
-> install Service and Credential Provider
-> clean installer temporary files after the setup app exits
-> open the post-install configuration panel when available
```

The user should not need to type `installer_cli` commands for normal setup.
Command-line tools remain the trusted backend and recovery surface.

## Product Requirement

WinFaceUnlock must support two frontend hosts:

```text
Standalone host:
  WinFaceUnlock can run independently and show its own setup/configuration UI.

External host:
  Another Windows application can embed the WinFaceUnlock configuration page in
  its own Windows Fluent frontend.
```

The external project's frontend style is out of scope for this repository. This
project must provide a reusable backend interface that both frontend hosts can
call.

## Technical Decision

Use **WinUI 3 / Windows App SDK** for the standalone graphical shell.

Rationale:

1. It gives the best native Windows experience for a Windows login/security
   tool.
2. It aligns with the external project's Windows Fluent frontend direction.
3. It avoids building a Web UI that would later be discarded for the external
   Windows frontend.
4. It keeps setup UI, Windows credential prompts, file pickers, progress UI, and
   recovery actions in a native desktop stack.

Accepted cost:

1. Windows App SDK packaging is more complex than a quick web shell.
2. Build and installer automation will need explicit handling.
3. The UI layer may use C#/.NET or C++/WinRT while the trusted backend remains
   Rust.

This is acceptable because the goal is the best long-term product path, not the
fastest prototype.

## Non-Goals

1. Do not copy the FaceWinUnlock-Tauri UI code. It is AGPL-3.0 and also uses a
   different UI stack.
2. Do not put registry, Service Control Manager, ACL, or Credential Provider
   registration logic inside the WinUI frontend.
3. Do not make the standalone UI's visual implementation the integration
   contract.
4. Do not pass Windows passwords through command-line arguments or temporary
   files.

## Architecture

```text
Frontend Host Layer
  Standalone WinUI 3 setup app
  External Windows Fluent app

Setup Backend Contract
  Stable command/IPC API used by both frontends

Rust System Backend
  installer_cli
  diagnostics_cli
  win_service
  windows_provider
```

The setup backend contract is the reusable boundary. Frontends can change; the
contract should stay stable.

## Repository Shape

Planned layout:

```text
apps/setup-winui/
  WinFaceUnlock.Setup.sln
  WinFaceUnlock.Setup.App/
  WinFaceUnlock.Setup.BackendClient/

crates/setup_api/
  Shared request/response DTOs for setup operations

crates/installer_cli/
  System install, repair, uninstall, emergency-disable

crates/diagnostics_cli/
  Credential enrollment, face enrollment, camera auth tests

crates/win_service/
  Runtime Windows Service

crates/windows_provider/
  Credential Provider DLL
```

`setup_api` is optional at first, but the DTOs should eventually live in Rust so
CLI, service, tests, and future IPC clients share the same schema.

## Switchable Frontend Host

When WinFaceUnlock is launched from a setup shortcut or installer, it should
choose a frontend host:

```text
1. Try external frontend host discovery with a short timeout.
2. If a compatible host responds, ask it to open the WinFaceUnlock setup page.
3. If no compatible host responds, start the standalone WinUI 3 setup app.
```

Recommended discovery transport:

```text
Named pipe:
  \\.\pipe\WinFaceUnlock.UiHost.v1
```

This pipe only routes UI ownership. It must not carry passwords, credential
material, registry writes, or authentication grants.

The external host protocol:

```json
{
  "type": "open_setup_page",
  "protocol_version": 1,
  "correlation_id": "...",
  "mode": "install | repair | configure | uninstall",
  "suggested_install_dir": "C:\\WinFaceUnlock"
}
```

The external app decides how to render the page. It then calls the same setup
backend operations as the standalone app.

## Setup Backend Operations

Both frontend hosts need the same capabilities:

```text
get_status
run_preflight
choose_install_dir
stage_payload
enroll_credential
enroll_face_template
run_auth_self_test
install_system_components
configure_presence_lock
repair
emergency_disable
uninstall
```

Each operation returns a structured result:

```json
{
  "operation": "install_system_components",
  "correlation_id": "...",
  "status": "succeeded | failed",
  "message": "...",
  "safe_details": {}
}
```

No password or plaintext credential material may appear in `message` or
`safe_details`.

## Backend Transport Options

### MVP Transport

Use `installer_cli` and `diagnostics_cli` with machine-readable JSON output and
structured exit codes.

Why:

1. The commands already own the verified privileged logic.
2. It is easy for WinUI, external apps, and tests to call.
3. It avoids designing a long-running setup service too early.

### Future Transport

Add a local named-pipe setup backend if command execution becomes too limiting:

```text
\\.\pipe\WinFaceUnlock.SetupBackend.v1
```

This can support streaming progress, cancellation, and richer validation.

## Installer State Model

The standalone WinUI shell exposes user-facing actions:

```text
Welcome
Disclaimer
Install location
Installing
Complete
```

The setup executable is only an installer. It must not expose face enrollment,
Windows credential binding, camera selection, threshold tuning, repair,
uninstall, or authentication self-test controls. Those belong to the
post-install configuration panel or recovery tools.

```text
Welcome
-> DisclaimerAccepted
-> ChooseInstallDir
-> InspectPayload
-> PreflightCheck
-> CopyPayload
-> InstallSystemComponents
-> InstallerCleanup
-> Complete
```

Failure states:

```text
PreflightFailed
CopyFailed
InstallFailed
CleanupWarning
```

The UI should render these states. The backend owns their truth.

## Command Contracts

### Install

The backend ultimately performs:

```powershell
installer_cli.exe install `
  --service-binary <InstallDir>\win_service.exe `
  --provider-binary <InstallDir>\windows_provider.dll `
  --start-service
```

Local camera authentication is configured later by the post-install
configuration panel. That later flow may call `configure-service-auth`,
`enroll_credential`, `enroll_face_template`, and `run_auth_self_test`, but those
are not setup wizard pages.

Presence lock remains independent:

```powershell
installer_cli.exe configure-presence-lock `
  --enable-presence-lock `
  --presence-detector-kind opencv-dnn-person `
  --presence-tracking-mode continuous-low-fps `
  --presence-person-detector-model yolov8-onnx `
  --presence-person-model <InstallDir>\models\yolov8n.onnx
```

### Repair

```powershell
installer_cli.exe repair `
  --service-binary <InstallDir>\win_service.exe `
  --provider-binary <InstallDir>\windows_provider.dll `
  --start-service
```

### Emergency Disable

```powershell
installer_cli.exe emergency-disable
```

This must be exposed from:

1. standalone setup app
2. external Fluent app
3. Start Menu shortcut
4. recovery `.cmd` in install directory

### Uninstall

Default full uninstall:

```powershell
installer_cli.exe uninstall
```

Explicit data retention:

```powershell
installer_cli.exe uninstall --preserve-data
```

## Credential Enrollment

Preferred path:

1. Use the WinUI password input page as the primary user experience.
2. Pass only a one-shot named pipe descriptor and nonce through setup JSON.
3. Send password material through the local pipe, never through command-line
   interpolation, JSON plaintext, logs, or temp files.
4. Store it only through the encrypted credential store.
5. Drop or zero short-lived password buffers immediately after enrollment.
6. Keep the native Windows credential prompt as a fallback path.

The MVP may use a native WinUI password box only if:

1. the value is never logged
2. the value is never passed as a process argument
3. the value is never written to a temp file
4. backend handoff uses the named pipe secret transport
5. enrollment is followed by `service-camera-auth` self-test

## Payload

The installer payload includes:

```text
installer_cli.exe
diagnostics_cli.exe
win_service.exe
windows_provider.dll
winfaceunlock-payload.json
models/
  face_detection_yunet_2023mar.onnx
  face_recognition_sface_2021dec.onnx
  minifasnet_v2.onnx
  yolov8n.onnx
recovery/
  emergency-disable.cmd
  uninstall.cmd
  repair.cmd
```

Model files remain excluded from Git but must be present in the local release
payload directory before building the installer.

Payload staging uses two path layers:

```text
payload_root_dir:
  absolute directory where the package payload was extracted

payload file source_path / target_relative_path:
  relative paths such as models\face_detection_yunet_2023mar.onnx
```

The selected install directory is independent from the extraction directory.
For example, a user may extract the setup payload under `%TEMP%` and install to
`D:\Apps\WinFaceUnlock`; the backend resolves payload sources under
`payload_root_dir` and writes staged files under the selected install
directory.

The setup frontend should not hand-author the full file list. It should call
`inspect_payload` first, then pass the returned `stage_payload_files` into
`stage_payload` together with the chosen `install_dir` and `payload_root_dir`.
This keeps model changes local to the payload manifest.

Example `winfaceunlock-payload.json`:

```json
{
  "manifest_version": 1,
  "payload_files": [
    {
      "file_id": "installer_cli",
      "source_relative_path": "installer_cli.exe"
    },
    {
      "file_id": "win_service",
      "source_relative_path": "win_service.exe"
    },
    {
      "file_id": "windows_provider",
      "source_relative_path": "windows_provider.dll"
    },
    {
      "file_id": "payload_manifest",
      "source_relative_path": "winfaceunlock-payload.json"
    },
    {
      "file_id": "yunet_model",
      "source_relative_path": "models\\face_detection_yunet_2023mar.onnx"
    },
    {
      "file_id": "sface_model",
      "source_relative_path": "models\\face_recognition_sface_2021dec.onnx"
    },
    {
      "file_id": "minifasnet_model",
      "source_relative_path": "models\\minifasnet_v2.onnx"
    },
    {
      "file_id": "yolov8_person_model",
      "source_relative_path": "models\\yolov8n.onnx",
      "required": false
    }
  ]
}
```

## Observability

The setup flow writes safe structured logs:

```text
%ProgramData%\WinFaceUnlock\install-logs\<timestamp>.jsonl
```

Each event:

```json
{
  "correlation_id": "...",
  "operation": "install_system_components",
  "status": "running",
  "message": "...",
  "timestamp_unix_ms": 0
}
```

Do not log:

1. passwords
2. plaintext credential material
3. token/cookie values
4. full raw command output if it can include secrets

## Test Strategy

### Local

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

### Backend Contract

1. Unit-test setup request/response DTOs.
2. Validate JSON output for install, repair, emergency-disable, and uninstall.
3. Verify no operation response contains password material.

### VM

1. Build payload.
2. Run standalone WinUI setup app.
3. Confirm Service installed and running.
4. Confirm Provider registered.
5. Run `diagnostics_cli service-camera-auth`.
6. Run emergency-disable from UI and recovery script.
7. Run repair and verify Provider restored.
8. Run uninstall and verify Provider, Service, and ProgramData are removed.

### Real Machine Gate

Real machine install is allowed only after:

1. VM graphical install, repair, emergency-disable, and uninstall pass.
2. A recovery script is present and tested.
3. The setup UI can show command failure with actionable recovery guidance.

## Implementation Plan

1. Define `docs/SETUP_BACKEND_API_CONTRACT.md`.
2. Add JSON output mode to `installer_cli` and diagnostics commands needed by
   setup.
3. Add setup payload staging script.
4. Scaffold WinUI 3 / Windows App SDK standalone setup app.
5. Implement backend client in the WinUI app.
6. Implement external frontend host discovery protocol.
7. Implement install directory selection and preflight.
8. Implement the standard setup wizard pages: welcome, disclaimer, install
   location, installing, and complete.
9. Keep credential enrollment, face enrollment, repair, emergency-disable, and
   uninstall outside the setup wizard UI.
10. Package as `WinFaceUnlockSetup.exe`.
11. Validate in VM.

## Current Status

Completed in the backend and VM validation path:

1. `installer_cli setup-backend`.
2. `inspect_payload`.
3. `stage_payload`.
4. `install_system_components`.
5. Payload manifest and staging script.
6. Runtime DLL and model inclusion in the payload.
7. VM validation for Service install, Provider registration, hidden tile mode,
   local camera authentication, credential serialization, and successful unlock.

Started in the graphical setup path:

1. `apps/setup-winui/WinFaceUnlock.Setup.sln`.
2. `WinFaceUnlock.Setup.BackendClient`.
3. `WinFaceUnlock.Setup.App` WinUI 3 shell.
4. WinUI setup wizard pages for welcome, disclaimer, install location,
   installing, and complete.
5. `SetupFlowCoordinator` high-level install plan for component-only setup.
6. Setup bundle assembly script.
7. Local WinUI compilation through `dotnet build
    apps\setup-winui\WinFaceUnlock.Setup.App\WinFaceUnlock.Setup.App.csproj
    -c Debug -p:Platform=x64`.
8. Self-contained `win-x64` WinUI publish output for target machines without a
    preinstalled .NET runtime.
9. WinUI publish output includes application XAML resources:
    `App.xbf`, `MainWindow.xbf`, and `WinFaceUnlock.Setup.App.pri`.
10. VM smoke validation for the setup bundle:
    `get_status`, `inspect_payload`, `run_preflight`, and `stage_payload`
    succeeded against `C:\Temp\WinFaceUnlockSetupBundle\payload`.
11. VM interactive-session smoke launch of
    `WinFaceUnlock.Setup.App.exe`; the process stayed responsive and no
    `.NET Runtime` or `Application Error` events were recorded.
12. Single-file Rust bootstrapper packaging through
    `scripts\build_setup_package.ps1`.
13. Local and VM interactive-session smoke launch of
    `target\setup-package\WinFaceUnlockSetup.exe`; the bootstrapper extracted
    the embedded bundle and activated the WinUI setup window.
14. Final package validation script:
    `scripts\validate_setup_package.ps1`.
15. Local package validation from the final single-file executable:
    bootstrapper path validation, `inspect_payload`, source preflight,
    temporary `stage_payload`, and staged preflight all passed against the
    extracted payload.
16. Backend client unit test coverage for payload source path resolution.
17. Backend client unit test coverage for setup install flow ordering.
18. Setup UI hides the internal extracted payload directory from users and uses
    a modern native folder picker for the install directory.
19. The bootstrapper cleans its hash-scoped temp bundle after the setup app
    exits.

Not complete yet:

1. Post-install configuration panel for credential binding, face enrollment,
   camera configuration, and authentication self-test.
2. Full VM validation through the graphical WinUI app by clicking the setup
   flow in the console session.

## MVP Cut

The first deliverable is not a polished UI. It is a reusable setup backend plus
a native standalone shell:

```text
WinFaceUnlockSetup.exe
```

MVP scope:

1. stable setup backend API
2. install directory selection
3. payload copy/staging
4. component installation
5. setup logs
6. temp bundle cleanup after setup exits
7. VM validation

Deferred:

1. final external Fluent page implementation
2. multi-account enrollment
3. advanced camera preview design
4. Start Menu status dashboard
