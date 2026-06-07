# WinFaceUnlock Setup WinUI Host

This directory contains the native Windows setup shell for Phase 8.

The UI is intentionally thin:

```text
WinUI 3 shell
-> SetupBackendClient
-> installer_cli.exe setup-backend
-> Rust installer/service/provider code
```

The UI must not write registry keys, install services, register the Credential
Provider, or handle plaintext credentials through command-line arguments.

## Current Scope

Implemented:

1. native setup wizard shell
2. install directory selection
3. welcome, disclaimer, install location, installing, and completion pages
4. high-level setup flow coordination over `inspect_payload`, `run_preflight`,
   `stage_payload`, and `install_system_components`
5. setup progress that reaches 100% only after the installer-side cleanup step
6. backend client path resolution and flow-plan tests

Out of scope for this setup executable:

1. face enrollment
2. Windows credential binding
3. camera selection
4. match threshold tuning
5. authentication self-test

Those belong to the post-install configuration panel.

## Build Prerequisites

This host requires:

1. .NET SDK 9.x or compatible Visual Studio workload
2. Windows App SDK / WinUI build support
3. Rust payload built with `scripts\build_setup_payload.ps1`

## Expected Layout At Runtime

For the package build, the app runs from:

```text
app\
payload\
```

The WinUI app is launched from `app\WinFaceUnlock.Setup.App.exe` and discovers
the payload at `..\payload`.

For direct development runs, the payload directory contains:

```text
installer_cli.exe
diagnostics_cli.exe
win_service.exe
windows_provider.dll
winfaceunlock-payload.json
models\
recovery\
```

The UI defaults the payload root to its own directory, then checks `..\payload`.
During development it also tries to discover `target\setup-payload`.

## Build

After installing the required SDK/workload:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build_setup_winui.ps1
```

To assemble the WinUI app output with the generated Rust payload:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build_setup_bundle.ps1
```

The bundle script creates `target\setup-bundle` with separate `app` and
`payload` directories.

To create the single-file setup bootstrapper:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build_setup_package.ps1
```

The package script creates `target\setup-package\WinFaceUnlockSetup.exe`. The
bootstrapper extracts the embedded bundle to a hash-scoped directory under the
user temp folder, then launches `app\WinFaceUnlock.Setup.App.exe`. That payload
path is internal installer state and is cleaned after the setup app exits.

To validate the final single-file package without opening the UI:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\validate_setup_package.ps1
```

The validation script rebuilds the package, asks the bootstrapper to verify and
report its extracted paths, then runs the setup backend through
`inspect_payload`, source preflight, temp staging, and staged preflight.
