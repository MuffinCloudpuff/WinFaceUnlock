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
  - Starts a Service wake request from `Advise` by default, on a background worker.
  - Runs Provider wake requests on a background worker so LogonUI enumeration and tile selection are not blocked by camera capture or face matching.
  - Computes `GetCredentialCount` from explicit tile visibility and credential readiness state.
  - Defaults to hidden-tile mode: no WinFaceUnlock tile is shown before credential material is ready.
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
  - `TileVisibility = hidden-until-ready`
  - `AutoWakeOnAdvise = true`
  - `WakeAuthSource = local-camera`

## Important Security Boundary

WinFaceUnlock must never disable or replace Windows built-in sign-in fallback methods.
PIN, password, Windows Hello, and standard Windows credential providers remain the recovery
path in every phase. WinFaceUnlock Provider failures should only disable or degrade the
WinFaceUnlock path itself; they must not modify Windows Hello, password sign-in, PIN
configuration, or account policy.

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
`CredentialMaterialReady`, `WakeFailed`, and `Completed`. Provider wake requests fetch
`FetchCredentialMaterial`; receiving `CredentialMaterialReady` makes the next
`GetCredentialCount` return the default auto-logon credential. The default install mode is
no-click auto-logon: WinFaceUnlock stays hidden while the Service authenticates in the
background. If authentication fails, LogonUI remains on Windows' native PIN/password
surface because WinFaceUnlock never becomes the selected tile.

Tile visibility is a policy choice:

- Hidden tile mode: return zero credentials while nothing is ready, then return one
  default auto-logon credential only after authentication succeeds.
- Visible tile mode: show WinFaceUnlock as an explicit debugging login option and still
  support automatic logon once credential material is prepared. Use this only for VM/manual
  Provider diagnostics.

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

`AutoWakeOnAdvise` defaults to `true`, but wake/auth runs on a Provider background worker,
not on the LogonUI callback thread. This is paired with `TileVisibility=hidden-until-ready`:
successful authentication can auto-logon, while failed authentication leaves the native
PIN/password surface untouched. Use `--show-tile-before-ready` only for VM debugging where
you intentionally want a visible WinFaceUnlock tile.

Provider-side risk controls:

- Credential material is consumed after a successful serialization attempt, so the same
  in-memory Windows password cannot repeatedly trigger auto-logon.
- `Advise` and `SetSelected` must return quickly; camera/auth work runs in a background
  worker and uses an agile COM reference only to notify `CredentialsChanged` after the
  Service returns.
- Cold boot can load LogonUI before the Service pipe is ready. Provider transport failures
  therefore retry in the background for a short window instead of failing the first wake
  immediately.
- Wake failures enter a short retry cooldown instead of immediately looping through
  repeated `CredentialsChanged` / wake attempts.
- Debug output for credential material redacts plaintext password bytes and protected
  password blob contents.
- Failure status text explicitly directs the user back to PIN or password fallback.
- `emergency-disable-provider` removes only the Credential Provider enumeration key, so
  Windows stops loading WinFaceUnlock while the original sign-in providers remain intact.

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
  --minifasnet-model C:\WinFaceUnlock\models\minifasnet_v2.onnx `
  --minifasnet-max-spoof-frame-ratio 0.40 `
  --match-threshold 0.75 `
  --required-consecutive 2

.\installer_cli.exe service-auth-status
```

The registry-backed Service config lives under:

```text
HKLM\SOFTWARE\WinFaceUnlock\Service
```

Current PowerShell `$env:WINFACEUNLOCK_*` values still override registry values for
diagnostics and one-off debugging, but VM service validation should use the registry path.
The local-camera Service path requires MiniFASNet liveness acceptance before face-template
matching. Screen-rectangle geometry remains diagnostic-only and does not reject grants.
MiniFASNet spoof frames are accumulated dynamically inside each authentication window. A
single spoof frame does not immediately reject the window. When consecutive live face
matches produce an unlock candidate, the Service rejects that candidate if spoof frames are
more than 40% of the MiniFASNet-evaluated frames observed so far. Otherwise it unlocks
immediately without waiting for all 30 frames. If no unlock candidate is produced before
the 30-frame limit, the request fails with the latest observed failure reason instead of
running a separate end-of-window spoof-ratio rejection.

The Credential Provider automatically retries at most three Service authentication windows
after a face-auth failure. The tile status is refreshed between attempts. After the third
failure it stops retrying and leaves Windows PIN or password sign-in available.

```powershell
.\installer_cli.exe install-service --start
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll
.\installer_cli.exe provider-status
```

For Phase 6 cold-boot validation, the Service must be normal `AUTO_START`, not delayed
auto-start. `repair-service` writes this metadata. Verify with:

```powershell
sc.exe qc WinFaceUnlockService
reg query HKLM\SYSTEM\CurrentControlSet\Services\WinFaceUnlockService /v DelayedAutoStart
```

Expected:

```text
START_TYPE         : 2   AUTO_START
DelayedAutoStart   REG_DWORD    0x0
```

If the VM has no camera, install the Provider with simulated wake auth:

```powershell
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll --wake-source manual-test
```

For explicit visible-tile debugging in a VM, add `--show-tile-before-ready`. This is not
the preferred real-machine mode because selecting the WinFaceUnlock tile makes fallback UX
less predictable than the hidden background mode:

```powershell
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll --wake-source manual-test --show-tile-before-ready
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
2. Default hidden tile mode does not show WinFaceUnlock before authentication is ready.
3. Provider loading triggers a Service wake request without selecting a WinFaceUnlock tile.
4. Visible tile debugging mode shows a tile when `--show-tile-before-ready` is explicitly used.
5. Service receives the wake request.
6. `CredentialsChanged` refreshes LogonUI after Service response.
7. After credential material is ready, `GetCredentialCount` requests default auto-logon.
8. Automatic logon succeeds without clicking the WinFaceUnlock tile.
9. Manual PIN/password login still works.
10. Hidden tile mode does not show the tile before authentication is ready.
11. Hidden tile mode can still auto-logon after authentication succeeds.
12. Wrong password or rejected auth does not enter an infinite retry loop.
13. Uninstall restores the default Windows login surface.
14. `emergency-disable-provider` stops WinFaceUnlock Provider enumeration without touching
    Windows PIN, password, or Windows Hello settings.

Cleanup:

```powershell
.\installer_cli.exe uninstall-provider
.\installer_cli.exe emergency-disable-provider
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
