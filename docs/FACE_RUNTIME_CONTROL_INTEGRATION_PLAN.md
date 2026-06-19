# Face Runtime Control Integration Plan

## Purpose

This document defines how the existing WinFaceUnlock face recognition and face
enrollment capabilities should be exposed to the official control frontend.

The goal is not to redesign the current Tauri UI. The goal is to make existing
backend capabilities available through a regular runtime control contract, then
let the current frontend call those operations from its existing user events.

This document is the execution plan for the next face-related integration work.

## Current Situation

WinFaceUnlock already has several real backend capabilities:

1. `diagnostics_cli` can run guided face enrollment and produce
   `selected_templates.json` plus an enrollment report.
2. `diagnostics_cli` can run local camera recognition and service camera auth
   diagnostics.
3. `installer_cli setup-backend` currently exposes transitional setup
   operations:
   - `enroll_face_template`
   - `run_auth_self_test`
4. `win_service` already consumes a configured face template path for local
   camera wake authentication.
5. `credential_store` already has repository-level records for face templates
   and user-to-template links, but the active service auth path still reads a
   selected template file from service configuration.

Important interpretation:

The capabilities exist, but the official runtime control API is not complete
yet. Therefore the next task is to expose the backend contract first, not to
wire React directly to `diagnostics_cli`, `setup_backend`, or template files.

## Boundary Decision

The official control path for face features is:

```text
React UI event
-> Tauri command adapter
-> runtime control protocol
-> control backend
-> face runtime integration
-> diagnostics/service/config/credential-store boundary
```

The frontend must not own:

1. template file paths
2. template JSON format
3. `diagnostics_cli` arguments
4. service registry keys
5. camera/auth policy decisions
6. credential store table layout

The setup backend may remain as a compatibility/reference implementation while
runtime control catches up, but new official control-panel work should not
extend the setup protocol.

## Integration Principles

1. Add backend semantic operations first.
2. Keep operation names independent of frontend labels.
3. Keep every request versioned and correlated.
4. Return only safe summaries, never raw face images, embeddings, encrypted
   template bytes, or full template JSON.
5. Treat the current frontend as replaceable.
6. Trigger backend calls only from explicit user actions or from opening a view
   whose purpose is to show backend state.
7. Do not add periodic polling unless the product deliberately adds a live
   progress/status surface.
8. Do not change the visual UI while wiring backend behavior unless the user
   explicitly asks for UI changes.

## Runtime Operations

The face runtime control surface should be split into three groups.

### Face Template Management

Operations:

```text
list_face_templates
delete_face_template
```

Purpose:

Expose the currently enrolled face template summaries to the control frontend
and allow explicit removal through a backend-owned operation.

Initial response shape:

```json
{
  "templates": [
    {
      "face_template_ref": "active-selected-template",
      "user_id": "dev-user",
      "display_name": "Leo16",
      "template_kind": "selected_template_set",
      "recognition_model": {
        "model_family": "opencv_sface",
        "model_version": "2021dec"
      },
      "selected_template_count": 5,
      "created_at_unix_ms": null,
      "updated_at_unix_ms": null,
      "source_state": "active_service_template"
    }
  ]
}
```

Initial delete request shape:

```json
{
  "face_template_ref": "active-selected-template"
}
```

Delete response shape:

```json
{
  "face_template_ref": "active-selected-template",
  "template_deleted": true,
  "service_auth_requires_reconfiguration": true
}
```

Required error distinctions:

```text
invalid_face_template_request
face_template_store_unavailable
face_template_not_found
active_template_delete_blocked
face_template_delete_failed
permission_denied
```

Implementation note:

The first implementation may summarize the active service template file
configured in service auth config, because that is what `win_service` consumes
today. It must still expose a stable backend-owned `face_template_ref` instead
of leaking the file path to React. If the repository-backed multi-template
store becomes the active runtime source later, the frontend contract should not
need to change.

### Face Enrollment

Operations:

```text
start_face_enrollment
get_face_enrollment_status
cancel_face_enrollment
finish_face_enrollment
```

Purpose:

Expose guided enrollment as a backend-owned stateful flow. The frontend starts
or cancels enrollment from existing user events and renders the backend state.

Start request shape:

```json
{
  "user_id": "dev-user",
  "camera_id": "opencv-index:0",
  "enrollment_profile": "guided_standard",
  "allow_partial_enrollment": false
}
```

Start response shape:

```json
{
  "enrollment_session_id": "face-enrollment-...",
  "session_state": "running",
  "user_id": "dev-user",
  "camera_id": "opencv-index:0",
  "current_step": null,
  "accepted_sample_count": 0,
  "required_sample_count": null
}
```

Status response shape:

```json
{
  "enrollment_session_id": "face-enrollment-...",
  "session_state": "running",
  "current_step": "frontal",
  "current_instruction_code": "look_at_camera",
  "accepted_sample_count": 3,
  "required_sample_count": 6,
  "last_frame_result": "pose_not_ready",
  "template_summary": null
}
```

Finish response shape:

```json
{
  "enrollment_session_id": "face-enrollment-...",
  "session_state": "completed",
  "face_template_ref": "active-selected-template",
  "user_id": "dev-user",
  "template_summary": {
    "selected_template_count": 5,
    "rejected_sample_count": 1
  },
  "service_auth_configured": true
}
```

Session states:

```text
starting
running
waiting_for_face
waiting_for_pose
capturing
finishing
completed
failed
cancelled
```

Required error distinctions:

```text
invalid_face_enrollment_request
face_enrollment_already_running
face_enrollment_session_not_found
camera_unavailable
face_model_unavailable
face_enrollment_failed
face_enrollment_cancelled
permission_denied
```

Implementation note:

`diagnostics_cli guided-enroll` is currently a batch command. A proper runtime
control flow needs either a backend session runner that can expose state, or a
controlled transitional wrapper that starts the batch enrollment and returns
coarse states. The first official API should still use the stateful operation
names above so the contract does not get stuck as a Tauri-only command wrapper.

### Authentication Self-Test

Operation:

```text
run_face_auth_self_test
```

Purpose:

Let the control panel explicitly trigger one real recognition/auth test after
credentials and face templates exist.

Request shape:

```json
{
  "session_id": "control-auth-self-test-...",
  "require_credential_ready": true,
  "camera_id": "opencv-index:0"
}
```

Response shape:

```json
{
  "session_id": "control-auth-self-test-...",
  "auth_match_passed": true,
  "grant_issued": true,
  "credential_material_ready": true,
  "credential_decryption_succeeded": true,
  "pipe_delivery_confirmed": true,
  "best_match_score": 0.81,
  "matched_face_template_ref": "active-selected-template"
}
```

Required error distinctions:

```text
invalid_auth_self_test_request
service_unavailable
camera_unavailable
face_template_missing
credential_missing
auth_match_failed
grant_issue_failed
credential_material_unavailable
auth_self_test_failed
```

The response must not collapse all of these into a generic `success` boolean.
The goal is to know which layer failed: recognition, grant issuance,
credential readiness, decryption, or IPC delivery.

## Data Ownership

### Active Template File

Today, the service auth path uses a configured template file such as:

```text
C:\WinFaceUnlock\selected_templates.json
```

Runtime control may summarize and replace that file through backend-owned
operations. React must never read or write this file directly.

### Repository Face Templates

The credential store already has:

```text
face_templates
user_face_templates
```

Those records are the better long-term source for multi-template management.
The runtime protocol should be designed so moving from active-template-file
storage to repository-backed storage does not affect the frontend.

### User Binding

Face operations should use the same backend-owned user identity as credential
binding:

```text
get_windows_credential_account
```

The frontend should not invent `user_id`, username, or account display names.

## Observability

Every operation must carry and log:

```text
correlation_id
operation
user_id
session_id when applicable
face_template_ref when applicable
operation_status
control_error_code when applicable
```

Logs must not contain:

1. raw face images
2. face embeddings
3. encrypted template bytes
4. plaintext credentials
5. full template JSON
6. secret pipe nonces after validation

For enrollment and self-test, backend logs should record state transitions:

```text
enrollment_started
camera_opened
pose_step_started
sample_accepted
template_generated
template_persisted
enrollment_completed
auth_self_test_started
auth_match_evaluated
grant_issued
credential_material_checked
auth_self_test_completed
```

## Implementation Roadmap

### Phase F0: Contract Documentation

Done when this document exists and the team agrees that face runtime work must
go through the runtime control protocol.

No frontend or backend behavior changes are required in this phase.

### Phase F1: Protocol Types

Add to `crates/control_protocol`:

```text
ListFaceTemplates
DeleteFaceTemplate
StartFaceEnrollment
GetFaceEnrollmentStatus
CancelFaceEnrollment
FinishFaceEnrollment
RunFaceAuthSelfTest
```

Add typed payloads and safe response detail structs for:

```text
FaceTemplateList
FaceTemplateSummary
DeleteFaceTemplatePayload
DeleteFaceTemplateOutcome
FaceEnrollmentStartPayload
FaceEnrollmentSessionStatus
FaceEnrollmentFinishOutcome
FaceAuthSelfTestPayload
FaceAuthSelfTestOutcome
```

Acceptance:

```powershell
cargo test -p control_protocol
```

### Phase F2: Backend Traits and Stubbed Handler

Add backend trait boundaries in `crates/control_backend`:

```text
FaceTemplateManagementStore
FaceEnrollmentRuntime
FaceAuthSelfTestRunner
```

The first handler implementation should validate payloads and return explicit
semantic errors for unimplemented runtime integrations. Tests should prove that
the protocol shape and error mapping are stable before touching React.

Acceptance:

```powershell
cargo test -p control_backend
```

### Phase F3: Face Template Listing

Implement `list_face_templates` against the currently active service template
configuration.

Minimum backend behavior:

1. read the service auth template path from backend-owned config
2. parse only safe summary fields from `selected_templates.json`
3. return a stable `face_template_ref`
4. hide the actual file path from React
5. distinguish missing config, missing file, parse failure, and empty template

Frontend behavior:

The existing face-management area may read this once when the settings view is
opened, because that area exists to show enrolled faces. It should render the
same existing visual surface using backend data.

### Phase F4: Face Auth Self-Test

Implement `run_face_auth_self_test` by adapting the existing service camera
auth path. If the first implementation shells out to packaged diagnostics, keep
that inside the backend integration boundary and mark it as transitional.

Frontend behavior:

Only an explicit user action may trigger this operation. No background self-test
or connection check should be added.

### Phase F5: Enrollment Session MVP

Implement `start_face_enrollment` and `cancel_face_enrollment` with clear
single-flight protection.

If the existing guided enrollment remains batch-oriented, the first session MVP
may expose coarse states:

```text
starting
running
completed
failed
cancelled
```

Do not fake fine-grained pose progress until the backend can actually report it.

Implemented backend behavior:

1. `control_backend` now owns a command-backed enrollment session runtime.
2. `start_face_enrollment` creates a backend session and starts the existing
   guided enrollment runtime behind a backend adapter.
3. The first runtime exposes only honest coarse states: `running`,
   `completed`, `failed`, and `cancelled`.
4. single-flight protection blocks a second running enrollment session.
5. `cancel_face_enrollment` cancels the owned backend process for the session.
6. the frontend contract does not expose runtime executable paths, template
   file paths, command arguments, embeddings, or raw template JSON.

### Phase F6: Enrollment Status and Finish

Expose real `get_face_enrollment_status` and `finish_face_enrollment` once the
backend enrollment runner owns session state.

This phase should also define whether finishing enrollment automatically
configures service auth to use the new template, or whether that remains a
separate explicit operation.

Implemented backend behavior:

1. `get_face_enrollment_status` refreshes the backend-owned session and returns
   the current coarse state.
2. `finish_face_enrollment` succeeds only after the session has completed and
   the generated selected-template file can be summarized safely.
3. the finish response returns a stable generated template reference and safe
   template counts only.
4. `service_auth_configured` is `true` only after the backend has applied the
   generated selected-template file as the active service-auth template. If the
   service configuration cannot be updated, finish must fail instead of
   reporting a completed enrollment.

### Phase F7: Delete Template

Implement `delete_face_template` only after the active-template-file versus
repository-template ownership rule is settled.

For the active service template, deletion may need to be guarded because it can
leave authentication unconfigured. The response must tell the frontend whether
service auth now requires reconfiguration.

## Recommended Immediate Next Step

Do not start with enrollment UI wiring.

Start with:

```text
F1 protocol types
F2 backend trait + error mapping
F3 list_face_templates
```

Reason:

Template listing is read-only, low-risk, and proves the contract without
starting the camera or modifying auth state. After that, `run_face_auth_self_test`
is the best second real integration because it exercises the existing
recognition/auth chain from an explicit event.

Enrollment should come after the read-only and self-test paths are stable,
because it has camera ownership, cancellation, session state, and persistence
semantics.

## Current Execution Status

The current codebase has started this plan and has completed the first backend
slice:

```text
F1 protocol types: completed
F2 backend trait + handler + error mapping: completed
F3 list_face_templates backend integration: completed
F4 run_face_auth_self_test: completed
F5/F6 enrollment session: completed for the coarse command-backed MVP
F8 existing React event wiring: completed for existing start/cancel/list/delete events,
including automatic finish after backend enrollment completion
```

Implemented scope:

1. `control_protocol` defines the face operations and typed safe payload /
   response structures.
2. `control_backend` owns face-template management, face-enrollment, and
   auth-self-test trait boundaries plus semantic handler errors.
3. `control_status` reads the active service template file and returns only a
   safe template summary for `list_face_templates`.
4. The Tauri command adapter uses the runtime control handler with the real
   face-template list backend. React UI components are not changed in this
   slice.
5. `run_face_auth_self_test` uses a backend-owned service IPC runner. It sends
   `WakeAuth(LocalCamera)` and then `FetchCredentialMaterial`, so the response
   can distinguish auth match, grant issuance, credential material readiness,
   credential decryption, and IPC delivery. React still does not know the pipe
   name or service request details.
6. `start_face_enrollment`, `get_face_enrollment_status`,
   `cancel_face_enrollment`, and `finish_face_enrollment` are connected to a
   backend-owned command session runtime. The Tauri adapter stores that runtime
   as application state so a start/status/cancel/finish sequence addresses the
   same backend session.
7. `finish_face_enrollment` summarizes generated templates through the same
   safe parser used by template listing. It does not return template paths,
   embeddings, full selected-template JSON, or runtime command details.

## React Wiring Scope

This wiring step connects only existing frontend events to backend-owned
runtime operations. It must not redesign the visual UI, add connection polling,
or expose diagnostics/runtime implementation details to React.

Existing event mapping:

1. opening the settings view loads safe face summaries through
   `list_face_templates`
2. clicking an existing face remove control sends `delete_face_template`
3. clicking the existing home enrollment start control sends
   `start_face_enrollment`
4. clicking the existing home enrollment cancel control sends
   `cancel_face_enrollment`
5. after a started enrollment session reaches `completed`, React calls
   `finish_face_enrollment`, exits enrollment mode, and refreshes the face
   template list through the backend adapter
6. selecting the existing keyboard trigger option sends
   `update_settings { logon_wake_mode: "input_triggered" }`

Current non-goals:

1. do not add global connection checks or background refresh timers
2. do not add a new finish-enrollment button; finish is the automatic backend
   completion step after a started enrollment session completes
3. do not add a new auth-self-test button until the product defines that event
4. do not wire React to template files, command arguments, pipe names, service
   registry keys, embeddings, or full template JSON

## Definition of Done

The face integration milestone is done when:

1. the runtime control protocol contains typed face operations
2. `control_backend` owns the face operation handlers
3. React calls only the Tauri adapter, not diagnostics/setup/file paths
4. the existing UI surface remains visually unchanged unless explicitly changed
5. face template listing is backed by real backend state
6. auth self-test runs through a real backend path
7. enrollment has backend-owned session state and cancellation
8. tests cover protocol serialization, invalid payloads, backend error mapping,
   and at least one successful read-only face management path

## Anti-Drift Checklist

Before each face integration change, verify:

1. Is this runtime control, not setup?
2. Would a WinUI frontend call the same operation?
3. Does the frontend avoid template paths and CLI arguments?
4. Does the operation return safe summaries only?
5. Are failures named by the layer that failed?
6. Is camera ownership explicit?
7. Is enrollment cancellation real or honestly reported as unsupported?
8. Is the current UI being reused rather than redesigned?
