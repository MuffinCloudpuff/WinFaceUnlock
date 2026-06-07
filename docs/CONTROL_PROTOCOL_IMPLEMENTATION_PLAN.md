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

Connect the official Tauri frontend to the runtime control protocol for status
only.

### Target Files

```text
apps/control-tauri/src-tauri/src/lib.rs
apps/control-tauri/src-tauri/src/backend/
apps/control-tauri/src/
```

### Tauri Command

```text
get_dashboard_status
```

The command should:

1. create a control request envelope
2. call the control backend handler
3. return a typed response or typed UI DTO
4. preserve `correlation_id` for troubleshooting
5. avoid leaking backend internals into React components

### Frontend Mapping

React should consume a small frontend model:

```text
dashboard.connection_state
dashboard.service.label
dashboard.service.detail
dashboard.provider.label
dashboard.config.label
dashboard.data.label
```

React must not directly understand registry keys, service manager internals,
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

Then, if the UI is touched, run the Tauri app and visually confirm that the
dashboard status renders without breaking the existing layout.

## Phase C3: Settings Read and Patch

### Goal

Add runtime settings after the status loop is stable.

### Operations

```text
get_settings
update_settings
```

### Settings Scope

Start with:

```text
presence_lock_enabled
presence_detector_kind
presence_tracking_mode
camera_id
match_threshold
```

Do not add face enrollment or credential binding to this phase.

### Acceptance

1. Settings can be read from backend state.
2. Partial updates preserve unrelated settings.
3. Validation failures return explicit error codes.
4. UI displays backend-confirmed state after save.

## Phase C4: Face Management

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

## Phase C5: Enrollment

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
2. The frontend renders backend state.
3. Progress events must use the same protocol family.
4. Do not introduce a Tauri-only enrollment protocol.
5. Camera ownership must be explicit before implementation.

## Work Order

Execute in this order:

```text
1. Phase C0: control_protocol
2. Phase C1: control_backend status handler
3. Phase C2: Tauri status adapter
4. Phase C3: settings
5. Phase C4: face management
6. Phase C5: enrollment
```

Do not start a later phase until the previous phase has a working test or
manual verification loop.

## Definition of Done for the First Milestone

The first milestone is complete when:

1. `control_protocol` exists and has serialization tests.
2. `control_backend` handles `get_dashboard_status`.
3. Tauri calls the runtime control path, not setup backend.
4. The React dashboard displays backend-derived status.
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
