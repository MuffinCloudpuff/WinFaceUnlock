# Logon Unlock IPC Blocking Analysis

Date: 2026-06-20

## Current Symptom

Installed WinFaceUnlock does not automatically unlock after lock screen input.

This is not currently explained by camera discovery, Credential Provider
registration, or the provider DLL version.

## Evidence

Installed provider registration points to the current package:

```text
HKLM\SOFTWARE\Classes\CLSID\{019E7C17-2BA4-74F1-879D-025113ECFD98}\InprocServer32
D:\tools\WinFaceUnlock\provider\windows_provider-55a89de81280.dll
```

The provider log shows LogonUI loads and invokes the provider:

```text
DllGetClassObject
ClassFactory.CreateInstance
Provider.SetUsageScenario
Provider.Advise
Provider.AutoWake
Provider.GetCredentialCount
Provider.AutoWakeInputAlreadyRecent
```

The same log later shows provider-to-service IPC failure:

```text
Provider.PipeConnectFailed pipe=\\.\pipe\winfaceunlock.service error=TransportUnavailable win32_error=231(ERROR_PIPE_BUSY)
Provider.WakeTransportRetry attempt=1/20 error=TransportUnavailable
```

Desktop diagnostics also fails to connect to the service pipe:

```text
WinFaceUnlockService pipe-check: TransportUnavailable win32_error=Some(5)
```

`5` is `ERROR_ACCESS_DENIED`.

## Direct Trigger

The Credential Provider reaches the auto-wake path, but it cannot complete the
provider-to-service request/response sequence because the service named pipe is
not accepting a normal client connection.

Observed failures:

- `ERROR_PIPE_BUSY` from LogonUI/provider context.
- `ERROR_ACCESS_DENIED` from desktop diagnostics context.

## Root Cause Hypothesis

The service IPC host currently owns a single long-lived `NamedPipeServer`
instance. `WakeAuth(local-camera)` is handled synchronously inside that IPC
request path.

That means one local-camera authentication attempt can occupy the only pipe
instance while it opens the camera, loads models, reads frames, runs liveness,
and matches templates.

If that first request is slow or stuck, follow-up requests cannot proceed. This
is especially bad for the Credential Provider path because the provider needs
two service interactions:

1. `WakeAuth`
2. `FetchCredentialMaterial`

If `WakeAuth` occupies the single pipe for too long, retries or the follow-up
fetch path see a busy/unavailable pipe instead of a structured auth result.

## Design Direction

Keep the Credential Provider small. It should only:

- react to LogonUI callbacks,
- request authentication,
- react to service events,
- serialize credentials after the service confirms readiness.

Move blocking camera work behind a service-owned job boundary.

Recommended service-side shape:

```text
Provider
  -> named pipe request: StartLogonAuth(session_id, source)
Service IPC
  -> records/returns AuthStarted quickly
  -> background auth worker owns camera/model work
Provider
  -> polls/subscribes FetchCredentialMaterial(session_id)
Service
  -> returns Pending/AuthFailed/CredentialMaterialReady
```

## Minimal Implementation Plan

1. Fix service IPC availability.
   - Do not let one long camera authentication monopolize the only pipe server.
   - Either create a new pipe instance per accepted connection or move accepted
     requests into worker threads and immediately recreate the listening pipe.

2. Add a service auth job state.
   - Key by `session_id`.
   - States should be explicit: `not_started`, `running`, `auth_failed`,
     `credential_material_ready`, `expired`.
   - Keep one active local-camera job at a time to avoid camera contention.

3. Change provider flow to tolerate pending auth.
   - `WakeAuth` should return quickly with a structured pending/started result,
     or provider should call a separate status/fetch request that can return
     `pending`.
   - Provider retries should not create multiple overlapping camera jobs.

4. Add service-side observability.
   - Log auth job start/end.
   - Log camera id, camera open duration, model load duration, frame count,
     last rejection reason, final auth result, and timeout.
   - Logs should follow the install directory, e.g.
     `D:\tools\WinFaceUnlock\logs\service.log`.

## Non-Goals

- Do not move camera/model code into `windows_provider.dll`.
- Do not fix this by only increasing retry count or sleep duration.
- Do not open the pipe to `Everyone`.
- Do not introduce a complex external queue. A small in-process job state is
  enough for this local service.

## Validation Plan

After implementation:

1. `diagnostics_cli pipe-check` must succeed while no auth is running.
2. `diagnostics_cli health-check` must succeed while an auth job is running.
3. A local-camera auth attempt must not block a second pipe connection.
4. Provider log must show either `CredentialMaterialReady` or a structured
   `AuthFailed` reason, not only transport failures.
5. Lock screen test must show no new `LogonUI.exe` crashes and no repeated
   `ERROR_PIPE_BUSY` provider retries.
