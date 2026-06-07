# Control Protocol Implementation Plan

## Purpose

This document defines the next development steps for connecting the official
Tauri control frontend to WinFaceUnlock backend capabilities without binding
the backend protocol to Tauri.

It follows the boundary defined in:

```text
docs/CONTROL_FRONTEND_PROTOCOL_ARCHITECTURE.md
```

The immediate goal is not to connect every screen. The immediate goal is to
establish the correct runtime control contract and prove it with one small
end-to-end feature.

## Non-Negotiable Boundary

The runtime control path is:

```text
React UI
-> Tauri command adapter
-> runtime control protocol
-> control backend
-> service / provider / config / credential-store integrations
```

The future WinUI 3 control path must be able to use the same protocol:

```text
WinUI 3 UI
-> WinUI backend client adapter
-> runtime control protocol
-> control backend
```

Do not route official runtime control features through `setup_backend`.

Setup remains responsible for installation lifecycle only:

```text
inspect_payload
run_preflight
stage_payload
install_system_components
repair
emergency_disable
uninstall
```

## Phase C0: Protocol Contract

### Goal

Create a frontend-independent runtime control contract.

### Target Crate

Preferred:

```text
crates/control_protocol
```

Acceptable alternative if it keeps a clean namespace:

```text
crates/common_protocol::control
```

### Initial Types

```text
ControlRequestEnvelope
ControlResponseEnvelope
ControlOperation
ControlOperationStatus
ControlErrorCode
DashboardStatus
ServiceStatusSummary
ProviderStatusSummary
ServiceConfigSummary
DataDirectorySummary
PresenceRuntimeSummary
```

### Initial Operation

```text
get_dashboard_status
```

### Required Rules

1. Every request carries `protocol_version`.
2. Every request carries `correlation_id`.
3. Operation names are frontend-independent.
4. Responses use semantic status values, not `ok` or `success` booleans.
5. Sensitive data is excluded from responses and test fixtures.
6. The crate has no Tauri dependency.
7. The crate has no WinUI/.NET dependency.
8. The crate has no Windows API dependency.

### Tests

Add unit tests for:

1. request envelope JSON round-trip
2. response envelope JSON round-trip
3. snake_case operation names
4. unsupported protocol response construction
5. dashboard status response shape

### Acceptance

```powershell
cargo test -p control_protocol
```

passes.

## Phase C1: Control Backend

### Goal

Implement the backend handler for `get_dashboard_status` without using
`setup_backend`.

### Target Crate

Preferred:

```text
crates/control_backend
```

### Initial Boundary

```text
ControlHandler
  handle(request: ControlRequestEnvelope) -> ControlResponseEnvelope

DashboardStatusProvider
  load_dashboard_status() -> Result<DashboardStatus, ControlBackendError>
```

### Integration Sources

The dashboard status can be assembled from existing backend responsibilities:

```text
Windows Service status
Credential Provider registration status
Service auth/config registry status
ProgramData / data directory status
Presence runtime status
```

Existing logic currently lives partly under `installer_cli` modules. Do not
copy that logic into Tauri. Prefer extracting shared Windows integration code
to a reusable backend crate if needed.

### Refactor Guideline

If `installer_cli` owns reusable primitives such as service status or provider
registry inspection, move those primitives to a shared crate with a neutral
name. Then let both `installer_cli` and `control_backend` depend on it.

Good names:

```text
crates/windows_integration
crates/system_integration
crates/control_backend
```

Avoid names that imply setup owns runtime control.

### Tests

Add unit tests for:

1. unsupported protocol handling
2. unknown operation handling
3. dashboard mapping from integration summaries
4. service missing state
5. provider partially registered state

Platform-specific Windows API calls should be behind small traits so the core
mapping can be tested without requiring a live Windows service.

### Acceptance

```powershell
cargo test -p control_backend
```

passes.

## Phase C2: Tauri Adapter

### Goal

Expose the runtime control protocol to the official Tauri frontend without
changing the established UI surface.

The Tauri frontend must call the backend only from explicit user events or
from a deliberately designed control workflow. Do not call the backend from
page mount just to check whether it is connected, and do not add periodic
status polling unless the product explicitly adds a live status surface.

### Target Files

```text
apps/control-tauri/src-tauri/src/lib.rs
apps/control-tauri/src-tauri/src/backend/
apps/control-tauri/src/
```

### Tauri Command

```text
handle_control_request
```

The command should:

1. accept a control request envelope from the frontend adapter
2. call the control backend handler
3. return the typed control response envelope
4. preserve `correlation_id` for troubleshooting
5. avoid leaking backend internals into React components
6. avoid owning frontend state or UI flow decisions

### Frontend Adapter

React should use a small adapter function:

```text
sendControlRequest(operation, payload)
```

Rules:

1. The adapter is a transport boundary, not a dashboard store.
2. The adapter must not start requests on module import or page mount.
3. A button click or form submit may call the adapter and use that response for
   that specific action.
4. Runtime status can still exist as a backend operation, but it must not be
   injected into the current UI unless a status surface is intentionally
   designed.
5. React must not directly understand registry keys, service manager internals,
   credential store paths, or pipe names.

### Tests and Checks

Run:

```powershell
cd apps\control-tauri
npm run lint
npm run build
cd src-tauri
cargo check --target x86_64-pc-windows-msvc
```

If a React component is touched, verify the visual surface still matches the
approved frontend. Also verify there is no automatic backend request on page
load and no timer-driven polling.

## Phase C3: Settings Read and Patch

### Goal

Add runtime settings through the same event-driven control contract.

### Operations

```text
get_settings
update_settings
```

### Settings Scope

Start with:

```text
presence_lock_enabled
logon_wake_mode = input_triggered
```

Only add a field when the frontend control maps to a real backend-owned
setting with clear persistence and runtime semantics. Do not wire UI controls
to placeholder settings that no backend component reads.

Candidate later settings, after their UI semantics are explicitly mapped:

```text
presence_detector_kind
presence_tracking_mode
camera_id
match_threshold
```

`logon_wake_mode` covers the future LogonUI unlock wake modes behind the
frontend labels "敲击键盘", "后台静默", and "智能混合". Its design and rollout
requirements live in:

```text
docs/LOGON_WAKE_MODES_DESIGN.md
```

`input_triggered` maps to the existing Provider-side input wake path and is the
only `logon_wake_mode` value that should be wired for now. Do not expose
`background_automatic` or `hybrid` until the low-frequency LogonUI scan
scheduler, cancellation, single-flight protection, cooldown, observability,
and VM validation are in place.

Do not add face enrollment to this phase. Credential binding belongs to the
separate event-driven credential enrollment phase below.

### Acceptance

1. Settings can be read from backend state.
2. Partial updates preserve unrelated settings.
3. Validation failures return explicit error codes.
4. A settings view may read settings once when opened because its purpose is
   to show backend state.
5. UI writes happen only from explicit user events and map the write response
   back into that specific control flow.
6. No global connection check, status strip, page-load dashboard request, or
   periodic polling is introduced.

## Phase C4: Credential Binding

### Goal

Bind the Windows credential used after successful face authentication through
the runtime control protocol.

### Operations

```text
enroll_windows_credential
```

### Rules

1. The frontend triggers the operation only from the account credential submit
   event.
2. The control envelope carries only safe metadata such as
   `windows_account_username`, `user_id`, `user_sid`, `account_type`, and
   `credential_ref`.
3. The password must not be serialized into the normal control request payload
   or any response `safe_details`.
4. A local adapter may pass the password through a one-shot secret side channel
   that is immediately consumed by the backend.
5. The backend owns credential store paths, default `user_id`, and
   `credential_ref` generation.
6. Responses distinguish invalid payload, missing secret, unavailable
   credential store, failed enrollment, and successful enrollment.

## Phase C5: Face Management

### Goal

Add list/delete face template management through the runtime control protocol.

### Operations

```text
list_face_templates
delete_face_template
```

### Rules

1. The frontend receives stable template references and display metadata.
2. The frontend does not infer database layout.
3. The frontend does not delete files directly.
4. Delete responses distinguish:
   - template deleted
   - template not found
   - permission denied
   - credential store unavailable
   - operation failed

## Phase C6: Enrollment

### Goal

Add a structured enrollment flow after status, settings, and face management
are stable.

### Operations

```text
start_enrollment
get_enrollment_status
cancel_enrollment
finish_enrollment
```

### Rules

1. Enrollment state belongs to the backend.
2. The frontend sends enrollment events through the adapter and renders only
   the state needed for that flow.
3. Progress events must use the same protocol family.
4. Do not introduce a Tauri-only enrollment protocol.
5. Camera ownership must be explicit before implementation.

## Work Order

Execute in this order:

```text
1. Phase C0: control_protocol
2. Phase C1: control_backend status handler
3. Phase C2: Tauri event-driven adapter
4. Phase C3: settings
5. Phase C4: credential binding
6. Phase C5: face management
7. Phase C6: enrollment
```

Do not start a later phase until the previous phase has a working test or
manual verification loop.

## Definition of Done for the First Milestone

The first milestone is complete when:

1. `control_protocol` exists and has serialization tests.
2. `control_backend` handles `get_dashboard_status`.
3. Tauri calls the runtime control path, not setup backend.
4. React exposes an event-driven protocol adapter without page-load polling.
5. Existing Tauri visual behavior remains unchanged.
6. The implementation can support a future WinUI 3 frontend by adding only a
   new frontend adapter.

## Anti-Drift Checks

Before each code change, ask:

1. Is this setup, runtime control, or auth IPC?
2. Would a WinUI 3 control frontend use the same contract?
3. Is this operation named after backend semantics instead of UI behavior?
4. Is Tauri only adapting the protocol, not owning business state?
5. Did we avoid extending `setup_backend` for runtime control?

If any answer is no, stop and adjust the design before coding.
