# Phase 5 LogonUI Wake Risk Fix

## Background

Phase 5 validates the Windows Credential Provider path in a VM. During VM testing, two
problems appeared after enabling no-click face sign-in:

1. `AutoWakeOnAdvise` being enabled by default made the Provider start camera
   authentication as soon as LogonUI enumerated the Provider.
2. The wake/auth request was executed synchronously inside Credential Provider callbacks,
   so LogonUI could feel slow or briefly stuck when the camera was blocked, unavailable,
   or slow to produce a usable frame.

These were Provider integration problems, not face-engine accuracy problems. PIN,
password, and Windows Hello fallback must remain available regardless of the WinFaceUnlock
state.

## Symptoms

- Lock screen entry became slower after enabling no-click auto wake.
- When the camera was covered and the user tried to switch to PIN or the normal Windows
  login UI, LogonUI still felt delayed.
- The visible tile and fallback path were present, but the Provider callback could still
  block user interaction long enough to make fallback feel unreliable.

## Root Cause

The first implementation called the Service wake path directly from:

- `ICredentialProvider::Advise`
- `ICredentialProviderCredential::SetSelected`

That wake path performs IPC to `WinFaceUnlockService`, and the Service may open the
camera, read frames, run face detection, run face recognition, and then fetch protected
credential material.

Those operations are not suitable for LogonUI callback threads. Even if the Service is
architecturally separate, the Provider was still waiting synchronously for the result.
When the camera path was slow or failed late, LogonUI had to wait too.

## Fix

### 1. Make Auto Wake Explicit

`AutoWakeOnAdvise` now defaults to `false`.

Default behavior:

- The WinFaceUnlock tile remains visible.
- Selecting the tile can start face sign-in.
- Windows PIN, password, and Windows Hello remain untouched.
- No camera/authentication work starts just because LogonUI enumerated providers.

No-click VM experiments can still enable the behavior explicitly:

```powershell
.\installer_cli.exe install-provider --provider-binary C:\WinFaceUnlock\windows_provider.dll --auto-wake-on-advise
```

### 2. Move Wake/Auth Work Off LogonUI Callbacks

`Advise` and `SetSelected` now call `request_wake_in_background` instead of running the
wake/auth path synchronously.

The Provider callback now does only lightweight work:

1. Attempts to transition Provider state into `WakeRequested`.
2. Starts a named background worker thread.
3. Immediately returns control to LogonUI.

The worker thread then:

1. Reads runtime Provider config.
2. Calls `WinFaceUnlockService` through local IPC.
3. Waits for authentication and protected credential material.
4. Updates Provider state.
5. Notifies LogonUI with `CredentialsChanged`.

This keeps Windows fallback interaction responsive while face recognition continues in
the background.

### 3. Use an Agile COM Reference for LogonUI Notification

`ICredentialProviderEvents` is a COM interface and cannot be moved directly across Rust
threads. The Provider now stores it as:

```rust
AgileReference<ICredentialProviderEvents>
```

The worker thread resolves the agile reference only when it needs to call
`CredentialsChanged`. This avoids unsafe cross-thread COM object sharing while still
allowing the background worker to refresh LogonUI after the Service returns.

### 4. Add Provider State Guards

The Provider state now also guards against secondary risks:

- A wake request cannot start again while one is already running.
- Failed wake attempts enter a short retry cooldown.
- Prepared credential material is consumed after serialization.
- Sensitive credential material is redacted from Debug output.
- Password UTF-16 buffers passed to `CredPackAuthenticationBufferW` are cleared after use.

## Why This Route

Moving camera and recognition into Provider would make the system more fragile because
LogonUI loads Provider DLLs in a sensitive process. The correct boundary is:

- Provider: thin Windows integration, state, tile, serialization, and notification.
- Service: camera, model inference, policy, credential store, and IPC handling.

The bug was not that recognition lived in the Service. The bug was that the Provider
waited synchronously for the Service. A background wake worker keeps the existing module
boundary while removing the user-visible LogonUI blocking.

Using `AgileReference` is preferable to unsafe manual COM pointer sharing because it makes
the cross-thread notification contract explicit and compile-time checked by the
`windows-core` abstraction.

## Validation

Local validation:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo build -p windows_provider -p installer_cli -p win_service -p diagnostics_cli
```

VM validation after deploying the fix:

```text
WinFaceUnlockProvider registered: true
WinFaceUnlockService status: Running
WinFaceUnlockService health-check: HealthOk
TileVisibility: visible
AutoWakeOnAdvise: true
WakeAuthSource: local-camera
```

Expected manual VM behavior:

- With `AutoWakeOnAdvise=true`, LogonUI may start face auth automatically.
- LogonUI should remain responsive while face auth runs.
- Covering the camera should not block switching to PIN/password fallback.
- Failed face auth should not enter an immediate retry loop.

## Recovery Rule

If Provider behavior becomes unstable in the VM, disable only WinFaceUnlock Provider
enumeration:

```powershell
.\installer_cli.exe emergency-disable-provider
```

This must not modify Windows PIN, password, Windows Hello, account policy, or other
Credential Providers.
