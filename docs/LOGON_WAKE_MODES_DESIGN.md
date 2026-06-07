# Logon Wake Modes Design

## Purpose

This document defines the future login and unlock wake modes behind the
control-panel UI option named "人脸识别触发方式".

This feature belongs to the LogonUI / Credential Provider unlock path. It is
not Presence Lock.

Presence Lock means:

```text
user is already logged in
-> service watches whether the user leaves the desk
-> service locks the session
```

Logon wake mode means:

```text
Windows is locked or waiting at LogonUI
-> WinFaceUnlock decides when to run face authentication
-> successful authentication prepares credential material
-> Credential Provider asks LogonUI to auto-logon / unlock
```

The frontend labels can stay product-oriented, but the backend contract must
use stable backend semantics rather than UI words.

## Target Modes

The runtime control setting should be named:

```text
logon_wake_mode
```

Allowed values:

```text
input_triggered
background_automatic
hybrid
```

UI mapping:

| UI label | Protocol value | Meaning |
| --- | --- | --- |
| 敲击键盘 | `input_triggered` | Do not scan until keyboard or mouse input is observed at LogonUI. Manual tile selection can also trigger immediately. |
| 后台静默 | `background_automatic` | While LogonUI is active, run low-frequency background face authentication even if no keyboard or mouse input occurs. |
| 智能混合 | `hybrid` | Run low-frequency background authentication, and also trigger or accelerate authentication when keyboard or mouse input is observed. |

## Current Implementation Status

Implemented today:

1. Provider wake work is moved to a background thread, so `Advise` and
   `SetSelected` return quickly.
2. Provider can trigger a wake request from `SetSelected` immediately.
3. Provider can start a wake worker from `Advise`.
4. The `Advise` wake worker currently waits for keyboard or mouse activity
   using `GetLastInputInfo` before calling the Service.
5. Runtime control can read and write `logon_wake_mode=input_triggered`.
6. Provider reads these registry values:

```text
HKLM\SOFTWARE\WinFaceUnlock\CredentialProvider

TileVisibility = visible | hidden-until-ready
AutoWakeOnAdvise = true | false
WakeAuthSource = local-camera | manual-test
LogonWakeMode = input-triggered
```

Important interpretation:

`AutoWakeOnAdvise=true` is currently a legacy provider configuration name. In
the current code path it starts a background worker from `Advise`, but that
worker uses `WaitForUserInputAfterAdvise` before performing the wake request.
So the current behavior is closest to `input_triggered`, not the final
`background_automatic` mode.

Not implemented yet:

1. Additional `logon_wake_mode` values beyond `input_triggered`.
2. A true low-frequency background scan loop for LogonUI unlock.
3. A hybrid scheduler that merges background scans with keyboard or mouse
   triggers.
4. VM validation for all three modes as user-selectable settings.

## Required Architecture Boundary

The ownership boundaries should be:

```text
Control frontend
-> writes logon_wake_mode through runtime control protocol

Runtime control backend
-> persists provider logon wake settings
-> returns structured settings snapshots

Credential Provider
-> owns LogonUI lifecycle
-> observes keyboard or mouse input when needed
-> starts, stops, or accelerates wake work
-> never blocks LogonUI callback threads

Service
-> owns camera, model inference, auth policy, Credential Store, and IPC
-> returns authentication and credential-material outcomes
```

The frontend must not know about:

```text
TileVisibility
AutoWakeOnAdvise
WakeAuthSource
GetLastInputInfo
CredentialsChanged
named pipe names
registry paths
```

Those are backend and platform details. The frontend should only select a
stable semantic mode.

## Proposed Persistence

Add a new Provider registry value:

```text
HKLM\SOFTWARE\WinFaceUnlock\CredentialProvider
LogonWakeMode = input-triggered | background-automatic | hybrid
```

The protocol should use snake_case enum values:

```json
{
  "logon_wake_mode": "input_triggered"
}
```

The registry can use kebab-case to match existing Windows-facing config style.
The control backend is responsible for mapping between the two.

Keep existing values for compatibility:

```text
TileVisibility
AutoWakeOnAdvise
WakeAuthSource
```

Compatibility rule:

1. If `LogonWakeMode` exists, it is authoritative.
2. If it is missing, derive legacy behavior from `AutoWakeOnAdvise`.
3. New runtime-control writes should write `LogonWakeMode`.
4. Do not expose `AutoWakeOnAdvise` directly to the frontend as the product
   setting.

## Mode Semantics

### input_triggered

Behavior:

1. LogonUI loads the Provider and calls `Advise`.
2. Provider arms an input observer.
3. Provider does not call Service until keyboard or mouse input changes.
4. When input is observed, Provider starts one wake worker.
5. Selecting the WinFaceUnlock tile also starts one immediate wake worker.
6. If authentication succeeds, Provider fetches credential material and calls
   `CredentialsChanged`.
7. If authentication fails, Windows PIN/password fallback remains available.

This is the first mode that should be wired to the control frontend because
the core implementation already exists.

Current code it can build on:

```text
WakeStartPolicy::WaitForUserInputAfterAdvise
wait_for_user_input_after_advise
GetLastInputInfo
Credential.SetSelected -> WakeStartPolicy::Immediate
```

Needed before exposing it:

1. Keep `logon_wake_mode = input_triggered` in the runtime control settings
   contract.
2. Keep Provider registry read/write for `LogonWakeMode`.
3. Keep Provider mapping from `LogonWakeMode=input-triggered` to
   `WakeStartPolicy::WaitForUserInputAfterAdvise`.
4. Keep `SetSelected` as an immediate wake path.
5. Verify in the VM that no wake happens before input, input triggers one wake,
   and fallback login remains unaffected.

### background_automatic

Behavior:

1. LogonUI loads the Provider and calls `Advise`.
2. Provider starts background authentication without waiting for keyboard or
   mouse input.
3. Authentication repeats on a controlled low-frequency cadence while LogonUI
   is active.
4. A successful match prepares credential material and triggers
   `CredentialsChanged`.
5. Failed attempts enter cooldown and must not loop aggressively.
6. `UnAdvise`, unlock, logoff, or provider unload stops background work.

This is not implemented as a complete independent mode yet.

Why the old immediate auto-wake is not enough:

1. A single wake on `Advise` is not the same as "scan every few seconds".
2. Repeated wake requests from Provider can fight with camera ownership,
   retries, and failure cooldown unless scheduled deliberately.
3. LogonUI must never wait on camera capture or model inference.
4. Failure must leave the user on PIN/password fallback.

Required backend work:

1. Add a cancellable logon wake session or scheduler.
2. Define scan cadence, cooldown, and attempt limits.
3. Ensure only one wake/auth operation owns the camera at a time.
4. Stop the loop reliably when Provider receives `UnAdvise`.
5. Report structured reasons for:
   - no face detected
   - face mismatch
   - liveness failed
   - camera unavailable
   - service unavailable
   - credential material unavailable
6. Add logs with correlation ids across Provider, IPC, Service, and auth
   attempts.

Recommended implementation shape:

```text
Provider Advise
-> start logon wake worker
-> worker owns mode scheduler
-> worker calls Service wake/auth at configured cadence
-> Service owns camera and auth
-> Provider applies result and calls CredentialsChanged on success
```

An alternative is a Service-owned long-running logon wake session:

```text
Provider StartLogonWakeSession
-> Service owns cadence and camera lease
-> Provider polls status or receives events
-> Provider StopLogonWakeSession on UnAdvise
```

The Service-owned session is cleaner long term, but it needs a larger auth IPC
extension. The Provider-owned scheduler is a smaller step if we keep strict
single-flight and cancellation rules.

### hybrid

Behavior:

1. LogonUI loads the Provider and background low-frequency authentication
   starts.
2. Keyboard or mouse input can immediately trigger or accelerate one attempt.
3. Background and input-triggered attempts must be de-duplicated.
4. A successful attempt from either source prepares credential material and
   unlocks through the normal Provider flow.
5. Failure cooldown applies across both sources.

This mode depends on `background_automatic`. It should not be implemented
first.

Required backend work:

1. All `background_automatic` requirements.
2. Input observer remains active while background scans are running.
3. Scheduler can accept an input boost signal.
4. Boost signal does not start a second concurrent camera/auth operation.
5. Metrics distinguish:
   - background attempt started
   - input boost received
   - input boost coalesced because an attempt is already running
   - auth succeeded by background attempt
   - auth succeeded by input-boosted attempt

## State Model

Provider-side state should remain explicit:

```text
ProviderLoaded
Armed
WaitingForInput
BackgroundScanScheduled
WakeRequested
CredentialPending
CredentialPrepared
AutoLogonRequested
Serialized
FailedFallback
Cooldown
Stopped
```

Rules:

1. Only one active wake/auth attempt per Provider instance.
2. Credential material is consumed after serialization.
3. Background workers stop when Provider loses the LogonUI event sink.
4. Failure never disables Windows PIN/password fallback.
5. Provider must avoid repeated `CredentialsChanged` loops on failure.

## Runtime Control Contract

Extend the existing settings payload later:

```rust
pub enum LogonWakeMode {
    InputTriggered,
    BackgroundAutomatic,
    Hybrid,
}

pub struct ControlSettingsSnapshot {
    pub presence_lock_enabled: bool,
    pub logon_wake_mode: LogonWakeMode,
}

pub struct ControlSettingsPatch {
    pub presence_lock_enabled: Option<bool>,
    pub logon_wake_mode: Option<LogonWakeMode>,
}
```

Do not expose these as UI-facing protocol values:

```text
keyboard
silent
hybrid
```

`hybrid` can exist as a backend value because it is a real backend mode name,
but the first two must not be UI words. The protocol should say what the
backend does, not what icon the frontend shows.

## Observability

Every wake attempt should carry or derive a correlation id that can be traced
through:

```text
Provider event log
auth IPC request
Service wake/auth handler
camera/auth attempt
credential material fetch
Provider state transition
CredentialsChanged notification
```

Important events:

```text
Provider.LogonWakeModeLoaded
Provider.InputWaitStarted
Provider.InputObserved
Provider.BackgroundScanScheduled
Provider.BackgroundScanStarted
Provider.InputBoostReceived
Provider.WakeAttemptStarted
Provider.WakeAttemptCoalesced
Provider.WakeAttemptCompleted
Provider.WakeCooldownStarted
Provider.WakeStopped
```

Metrics to sample in VM:

1. time from LogonUI load to first attempt
2. time from input to wake request
3. time from wake request to auth result
4. camera open duration
5. CPU and memory while locked
6. failed-attempt cooldown behavior
7. number of wake attempts per lock session

## Validation Plan

All three modes must be validated in a Windows VM snapshot before real-machine
use.

### input_triggered acceptance

1. Lock Windows.
2. Do not touch keyboard or mouse.
3. Verify no Service wake/auth attempt starts.
4. Press a key or move the mouse.
5. Verify exactly one wake/auth attempt starts.
6. If face matches, verify unlock succeeds.
7. If face fails, verify PIN/password fallback remains available.
8. Verify selecting the WinFaceUnlock tile triggers immediate wake.

### background_automatic acceptance

1. Lock Windows.
2. Do not touch keyboard or mouse.
3. Verify background attempts start on the configured cadence.
4. Verify LogonUI remains responsive during attempts.
5. Verify successful face match unlocks.
6. Verify repeated failures enter cooldown and do not spin.
7. Verify `UnAdvise` or unlock stops the loop.
8. Verify CPU, memory, and camera usage stay within agreed limits.

### hybrid acceptance

1. Lock Windows.
2. Verify background attempts are scheduled.
3. Move mouse or press key before the next scheduled attempt.
4. Verify the input signal triggers or accelerates one attempt.
5. Verify an input signal during an active attempt is coalesced, not duplicated.
6. Verify either background success or input-boosted success can unlock.
7. Verify failure cooldown applies across both sources.

## Recommended Roadmap

### Phase L1: Document and Contract

1. Add this document.
2. Add `logon_wake_mode` to runtime control design docs.
3. Keep the current frontend UI unchanged.

### Phase L2: Wire input_triggered

Status: implemented in runtime control and frontend settings for
`input_triggered`; still needs VM behavior validation.

Completed implementation items:

1. Add `LogonWakeMode` enum to `control_protocol`.
2. Extend settings snapshot and patch.
3. Add Provider settings read/write in the Windows settings store.
4. Add `LogonWakeMode` parsing to `windows_provider`.
5. Map frontend "敲击键盘" to `input_triggered`.

Remaining validation:

1. Verify no wake happens before input in a locked VM.
2. Verify keyboard or mouse input triggers one wake.
3. Verify Windows PIN/password fallback remains available after failure.

### Phase L3: Build background_automatic

1. Add a low-frequency wake scheduler.
2. Add cancellation on `UnAdvise`.
3. Add single-flight camera/auth protection.
4. Add cooldown and observability.
5. Validate in VM under blocked camera, no-face, wrong-face, and service-down
   cases.

### Phase L4: Build hybrid

1. Reuse the background scheduler.
2. Add input boost signals.
3. Coalesce duplicate attempts.
4. Add mode-specific metrics and VM acceptance tests.

## Non-Goals

This feature must not:

1. Change Presence Lock behavior.
2. Disable Windows PIN, password, or Windows Hello fallback.
3. Put camera capture or model inference inside the Provider DLL.
4. Block LogonUI callback threads.
5. Expose registry keys or Provider internals to frontend components.
6. Use a Tauri-only protocol for wake mode settings.
