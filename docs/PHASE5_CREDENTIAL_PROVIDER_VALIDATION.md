# Phase 5 Credential Provider Auto-Logon Validation

Phase 5 introduces the Windows Credential Provider boundary. All Winlogon loading,
registration, lock-screen, and auto-logon behavior must be validated in a Windows VM
snapshot before any real-machine installation.

The target user experience is not "click the WinFaceUnlock tile every time." The
Credential Provider tile is still the Windows authentication entry, but the Provider
should be able to wake the Service when LogonUI loads, receive an authentication result,
call `CredentialsChanged`, and then return a default auto-logon credential during the
next `GetCredentialCount` pass.

## Implemented Host-Side Scope

- `windows_provider` builds as `cdylib` and `rlib`.
- DLL exports:
  - `DllGetClassObject`
  - `DllCanUnloadNow`
  - `DllMain`
- COM class factory:
  - Rejects aggregation.
  - Creates `ICredentialProvider`.
- Credential Provider:
  - Supports `CPUS_LOGON`.
  - Supports `CPUS_UNLOCK_WORKSTATION`.
  - Implements `Advise` / `UnAdvise`.
  - Starts a Service wake request from `Advise` so LogonUI loading can trigger recognition without tile click.
  - Computes `GetCredentialCount` from explicit tile visibility and credential readiness state.
  - Exposes one basic tile.
- Credential tile:
  - Implements field state and string fields.
  - Triggers a local Service wake request when selected.
  - Requests a protected credential reference after an auth grant.
  - Calls `CredentialsChanged` after wake success, auth failure, protocol rejection, or transport failure.
  - Calls `CredPackAuthenticationBufferW` only when in-memory credential material is present.
- `installer_cli` commands:
  - `install-provider`
  - `uninstall-provider`
  - `provider-status`
  - `configure-service-auth`
  - `service-auth-status`
- Provider registry policy defaults:
  - `TileVisibility = visible`
  - `AutoWakeOnAdvise = true`

## Important Security Boundary

The Provider-to-Service IPC must not send a plaintext password as a normal protocol field.
Phase 5 now has two credential retrieval contracts:

```text
FetchCredential
  -> CredentialReady
  -> ProtectedCredential { credential_ref }

FetchCredentialMaterial
  -> CredentialMaterialReady
  -> ProtectedCredentialMaterial {
       domain,
       username,
       protected_password,
       protection = DpapiLocalMachineV1
     }
```

The Service decrypts the credential blob inside the Service process, then wraps only the
password bytes with DPAPI LocalMachine before sending it to the Provider. The Provider
unwraps this envelope inside LogonUI and immediately uses the material for
`CredPackAuthenticationBufferW`.

This is still sensitive and must stay restricted to the local named pipe ACL and one-time
grant redemption. Do not replace it with a plaintext pipe field.

## Auto-Logon Design Target

The Provider should follow this state model:

1. `ProviderLoaded`: LogonUI loads the Provider and calls `Advise`.
2. `WakeRequested`: the Provider asks the Service to start local face authentication.
3. `CredentialPending`: the Service is authenticating or fetching credential material.
4. `CredentialPrepared`: credential material is available in memory for one submission.
5. `AutoLogonRequested`: the Provider calls `CredentialsChanged`.
6. `AutoLogonEnumerated`: `GetCredentialCount` returns:
   - `pdwcount = 1`
   - `pdwdefault = 0`
   - `pbautologonwithdefault = TRUE`
7. `Serialized`: `GetSerialization` returns a packed credential.
8. `Completed` or `FailedFallback`: `ReportResult` clears temporary state and avoids
   infinite retry loops.

Current implementation has the state boundaries for `ProviderLoaded`, `WakeRequested`,
`CredentialMaterialReady`, `WakeFailed`, and `Completed`. Provider auto-wake now requests
`FetchCredentialMaterial`; receiving `CredentialMaterialReady` makes the next
`GetCredentialCount` return the default auto-logon credential.

Tile visibility is a policy choice:

- Visible tile mode: show WinFaceUnlock as an explicit login option and still support
  automatic logon once credential material is prepared.
- Hidden tile mode: return zero credentials while nothing is ready, then return one
  default auto-logon credential only after authentication succeeds.

Both modes must preserve Windows PIN/password fallback.

Current registry policy values live under:

```text
HKLM\SOFTWARE\WinFaceUnlock\CredentialProvider
```

Supported values:

```text
TileVisibility = visible | hidden-until-ready
AutoWakeOnAdvise = true | false
WakeAuthSource = local-camera | manual-test
```

`WakeAuthSource` defaults to `local-camera`. Use `manual-test` only in a VM that has no
camera passthrough, so Credential Provider loading, `CredentialsChanged`, credential
material retrieval, and serialization can be validated independently from the camera
pipeline.

## Host Build

```powershell
cargo build -p windows_provider -p installer_cli -p win_service -p diagnostics_cli
```

Expected Provider DLL:

```text
target\x86_64-pc-windows-msvc\debug\windows_provider.dll
```

## Dry Run

```powershell
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe install-provider --dry-run
.\target\x86_64-pc-windows-msvc\debug\installer_cli.exe provider-status
```

## VM Validation Order

Run only inside a VM with a fresh snapshot.

First enroll the real Windows credential into the local encrypted store. Do not send the
password through chat or command-line arguments; this command prompts twice with hidden
input:

```powershell
.\diagnostics_cli.exe enroll-windows-credential --username <WindowsUserName> --user-id dev-user --account-type local
```

Then persist the Service's local-camera auth configuration in HKLM so the Windows Service
can read it after it starts as LocalSystem:

```powershell
.\installer_cli.exe configure-service-auth `
  --face-template C:\WinFaceUnlock\phase4-face-template.json `
  --camera-id opencv-index:0 `
  --yunet-model C:\WinFaceUnlock\models\face_detection_yunet_2023mar.onnx `
  --sface-model C:\WinFaceUnlock\models\face_recognition_sface_2021dec.onnx `
  --match-threshold 0.55 `
  --required-consecutive 2

.\installer_cli.exe service-auth-status
```

The registry-backed Service config lives under:

```text
HKLM\SOFTWARE\WinFaceUnlock\Service
```

Current PowerShell `$env:WINFACEUNLOCK_*` values still override registry values for
diagnostics and one-off debugging, but VM service validation should use the registry path.

```powershell
.\installer_cli.exe install-service --start
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll
.\installer_cli.exe provider-status
```

If the VM has no camera, install the Provider with simulated wake auth:

```powershell
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll --wake-source manual-test
```

To inspect the protected material path without printing a password, use:

```powershell
.\diagnostics_cli.exe wake-auth --source local-camera --session-id phase5-material-test
.\diagnostics_cli.exe fetch-credential-material --session-id phase5-material-test --grant-id <grant_id> --nonce <nonce>
```

The diagnostics output must show `CredentialMaterialReady`, user/domain/username,
`DpapiLocalMachineV1`, and a nonzero `protected_password_bytes` value. It must not print the
password.

Then validate:

1. Lock screen still loads.
2. WinFaceUnlock visible tile mode shows a tile.
3. Selecting the tile does not crash LogonUI.
4. Provider loading can trigger a Service wake request without selecting the tile.
5. Service receives the wake request.
6. `CredentialsChanged` refreshes LogonUI after Service response.
7. After credential material is ready, `GetCredentialCount` requests default auto-logon.
8. Automatic logon succeeds without clicking the WinFaceUnlock tile.
9. Manual PIN/password login still works.
10. Hidden tile mode does not show the tile before authentication is ready.
11. Hidden tile mode can still auto-logon after authentication succeeds.
12. Wrong password or rejected auth does not enter an infinite retry loop.
13. Uninstall restores the default Windows login surface.

Cleanup:

```powershell
.\installer_cli.exe uninstall-provider
.\installer_cli.exe stop-service
.\installer_cli.exe uninstall-service
```

If the login surface becomes unstable, revert the VM snapshot first. If snapshot revert is
not available, boot recovery/safe mode and delete:

```text
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{019E7C17-2BA4-74F1-879D-025113ECFD98}
HKLM\SOFTWARE\Classes\CLSID\{019E7C17-2BA4-74F1-879D-025113ECFD98}
HKLM\SOFTWARE\WinFaceUnlock\CredentialProvider
HKLM\SOFTWARE\WinFaceUnlock\Service
```
