# Control Frontend Protocol Architecture

## Purpose

This document defines the architectural boundary for WinFaceUnlock control
frontends.

The control frontend can be implemented with Tauri, WinUI 3, a future native
Windows shell, or automated validation tools. Those frontends must share the
same runtime control protocol. The protocol is a backend contract, not a UI
implementation detail.

The current official control frontend direction is Tauri + React. This choice
must not make the backend protocol Tauri-specific.

## Decision

WinFaceUnlock uses separate contracts for separate domains:

```text
Setup frontend
-> setup protocol
-> installer / repair / uninstall backend

Control frontend
-> runtime control protocol
-> service / configuration / face-management backend

Credential Provider
-> auth IPC protocol
-> service authentication and credential grant backend
```

The setup protocol, runtime control protocol, and auth IPC protocol must not be
merged just because one frontend needs to show several screens.

## Frontend Independence Rule

All user-facing control frontends must depend on the same runtime control
contract:

```text
Tauri React UI
        |
        v
Tauri command adapter
        |
        v
runtime control protocol

WinUI 3 UI
        |
        v
WinUI backend client adapter
        |
        v
runtime control protocol
```

The frontend adapter may map UI state, local validation, progress display, and
localized text. It must not own backend truth, registry semantics, credential
store structure, face-template file formats, or authentication policy.

## Domain Boundaries

### Setup Domain

Setup is only responsible for installation lifecycle operations:

1. inspect payload
2. run preflight checks
3. stage payload
4. install system components
5. repair installation
6. emergency disable
7. uninstall

Setup must not be the long-term owner of these runtime controls:

1. face enrollment
2. face-template management
3. Windows credential binding after install
4. camera selection
5. recognition threshold tuning
6. presence-lock configuration after install
7. authentication self-test from the normal control panel
8. dashboard runtime status

If a setup contract contains runtime operations for a temporary phase, treat
that as transitional compatibility. Do not extend that path for the official
control frontend.

### Runtime Control Domain

Runtime control owns the post-install configuration panel and day-to-day
management experience.

Initial runtime control operations should include:

```text
get_dashboard_status
get_settings
update_settings
get_windows_credential_account
enroll_windows_credential
list_face_templates
delete_face_template
start_enrollment
cancel_enrollment
get_enrollment_status
run_auth_self_test
```

These operation names are examples of the intended contract shape. The final
Rust enum names can be adjusted, but the semantic boundaries must remain.

### Auth IPC Domain

Auth IPC is the Credential Provider to Service authentication protocol. It
currently covers requests such as:

```text
wake_auth
fetch_credential
fetch_credential_material
cancel
health_check
```

This protocol carries short-lived authorization state and protected credential
handoff. It must remain focused on authentication and must not become a general
UI control API.

## Contract Ownership

The runtime control contract should live in a backend-owned Rust crate, for
example:

```text
crates/control_protocol
```

or in a clearly separated module inside:

```text
crates/common_protocol
```

The chosen location must make these facts obvious:

1. the contract is shared by all frontends
2. Tauri is only one client
3. WinUI 3 can implement another client without changing the protocol
4. tests can call the same protocol without a UI
5. setup protocol changes do not imply runtime control protocol changes

## Transport

The runtime control protocol is independent of transport.

Allowed transports:

```text
Tauri command -> in-process Rust protocol client
WinUI client -> local named pipe client
test harness -> in-process protocol handler
```

Recommended long-term Windows transport:

```text
\\.\pipe\WinFaceUnlock.Control.v1
```

This pipe should be separate from the auth IPC pipe unless a later ADR proves
that sharing the transport is safer and clearer. The control pipe handles
configuration and management. The auth pipe handles Credential Provider
authentication grants and protected credential exchange.

## Request Envelope

Runtime control requests should be structured and versioned:

```json
{
  "protocol_version": 1,
  "correlation_id": "control-...",
  "operation": "get_dashboard_status",
  "payload": {}
}
```

Rules:

1. Every request carries a `correlation_id`.
2. Every operation uses a named payload type.
3. Public fields use explicit semantic names.
4. Avoid ambiguous names such as `success`, `ok`, `flag`, or generic
   `status` when multiple layers can succeed or fail independently.
5. Do not pass plaintext passwords, credential material, full face images, API
   keys, tokens, or unnecessary sensitive data.

### Credential Secret Channel

`get_windows_credential_account` returns the backend-owned Windows account
identity that the credential binding page should display and use for
subsequent credential enrollment. The frontend must not hard-code a display
name such as "Admin User" when a backend account profile is available.

`enroll_windows_credential` is a runtime control operation because it binds the
post-install Windows credential used after successful face authentication.

The control request envelope carries only safe enrollment metadata:

```json
{
  "protocol_version": 1,
  "correlation_id": "control-...",
  "operation": "enroll_windows_credential",
  "payload": {
    "windows_account_username": "Leo16",
    "user_id": "dev-user",
    "user_sid": "S-1-5-21-winfaceunlock-pending",
    "account_type": "local",
    "credential_ref": "windows-credential-dev-user"
  }
}
```

Rules:

1. The request payload must not contain a password field.
2. Tauri may pass the user-entered password as a local command argument that is
   immediately consumed by the Rust adapter.
3. A future WinUI or named-pipe control client must use an equivalent secret
   side channel, such as a one-shot pipe or native credential prompt.
4. `safe_details` may return username, `user_id`, account type, and
   `credential_ref`; it must not return plaintext or protected password bytes.
5. The frontend must trigger this operation only from an explicit user submit
   event.

## Response Envelope

Runtime control responses should also be structured:

```json
{
  "protocol_version": 1,
  "correlation_id": "control-...",
  "operation": "get_dashboard_status",
  "operation_status": "completed",
  "message": "Dashboard status loaded.",
  "safe_details": {},
  "next_recommended_action": null
}
```

`operation_status` should use explicit values such as:

```text
completed
failed
requires_elevation
requires_user_input
service_unavailable
permission_denied
invalid_request
unsupported_protocol
cancelled
```

Do not use a single boolean for cross-layer results. For example, a future auth
self-test response should separately report:

```text
auth_match_passed
grant_issued
credential_material_ready
credential_decryption_succeeded
pipe_delivery_confirmed
```

## Frontend Mapping

The frontend adapter maps backend state to UI-specific state:

```text
ControlResponse.safe_details
-> Tauri command return type
-> React store / component state
```

or:

```text
ControlResponse.safe_details
-> WinUI backend client DTO
-> ViewModel state
```

The mapping layer may rename fields for display ergonomics, but it must not
change the backend protocol or invent backend truth.

## Frontend Request Lifecycle

The control frontend must not treat backend reachability as something that is
continuously checked by default. A backend call should be tied to one of these
explicit causes:

1. the user clicks a control action
2. the user submits a form
3. the user opens a view whose purpose is to show backend state
4. the product explicitly defines a live status surface

For the current Tauri UI, the established visual shell should stay intact.
Backend integration is added behind existing events. Do not add status strips,
dashboard cards, background refresh timers, or page-load connection checks as
side effects of wiring backend APIs.

When a user action needs the backend, that action should send one semantic
operation, receive one structured response, and map only that response into the
current flow. Connection failure, unsupported protocol, permission failure, and
operation failure are action results, not separate global UI polling states.

## Current Codebase Status

Current implemented pieces:

1. `common_protocol` contains the auth IPC request/event protocol used by the
   Credential Provider and Service.
2. `ipc` contains named-pipe framing and request/response transport.
3. `win_service` hosts the auth IPC server and service runtime behavior.
4. `setup_api` and `installer_cli setup-backend` contain the setup protocol.
5. the old WinUI control app currently reads status through setup backend
   compatibility code.
6. the Tauri control app exposes `handle_control_request` as a runtime control
   adapter. Current UI integration should stay event-driven and must not add
   dashboard polling as a side effect.

Important interpretation:

The old WinUI control app's use of setup backend status is not the desired
runtime control architecture. It is a compatibility shortcut caused by the
runtime control protocol not being split out yet.

## Implementation Plan

### Phase C0: Contract Extraction

Create the runtime control contract without binding it to Tauri:

```text
ControlRequest
ControlResponse
ControlOperation
ControlOperationStatus
DashboardStatusPayload
SettingsSnapshot
SettingsPatch
FaceTemplateSummary
EnrollmentSessionState
```

Add unit tests for serialization, enum naming, invalid payload handling, and
redaction.

### Phase C1: Status Read

Implement:

```text
get_dashboard_status
```

The status should report service, provider, service configuration, data
directory, and presence runtime status through stable fields. The Tauri
frontend and any future WinUI frontend should consume the same response.

### Phase C2: Settings Read and Patch

Implement:

```text
get_settings
update_settings
```

Settings patches must be partial and semantic. For example:

```text
presence_lock_enabled
```

The first implementation should only expose `presence_lock_enabled`, because
that setting already has a real backend registry value and service runtime
behavior. Add more fields only after their frontend semantics are mapped to
backend-owned configuration:

```text
logon_wake_mode
presence_detector_kind
presence_tracking_mode
camera_id
match_threshold
```

`logon_wake_mode` is the backend-owned setting behind the frontend labels
"敲击键盘", "后台静默", and "智能混合". Only `input_triggered` is implemented
for now. The full three-mode design is defined in:

```text
docs/LOGON_WAKE_MODES_DESIGN.md
```

This is a LogonUI unlock wake feature, not Presence Lock.

The protocol should distinguish validation failure, permission failure,
service-unavailable failure, and successful persistence.

### Phase C3: Face Management

Implement:

```text
list_face_templates
delete_face_template
```

The response should return stable template references and display metadata. The
frontend must not infer database paths or delete template files directly.

### Phase C4: Enrollment

Implement enrollment as a stateful runtime control flow:

```text
start_enrollment
get_enrollment_status
cancel_enrollment
finish_enrollment
```

If progress streaming is needed, use control events over the same protocol
family. Do not introduce a Tauri-only enrollment protocol.

## Regression Checklist

Before adding or changing frontend/backend integration, verify:

1. Does this operation belong to setup, runtime control, or auth IPC?
2. Can a Tauri frontend and a WinUI 3 frontend both call the same backend
   contract?
3. Is the operation name independent of UI technology?
4. Are request and response fields structured and versioned?
5. Are failures represented by explicit semantic states?
6. Are secrets and sensitive data excluded from logs and responses?
7. Is the UI only mapping backend state rather than owning backend truth?
8. Would this design still make sense if the current Tauri frontend were
   replaced tomorrow?

If any answer is no, stop and fix the boundary before implementing.
