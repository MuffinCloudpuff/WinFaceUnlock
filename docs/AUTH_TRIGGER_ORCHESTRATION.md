# Auth trigger orchestration

## Problem

Unlock currently mixes four concerns:

- deciding when to request authentication
- deduplicating concurrent authentication requests
- running camera face authentication
- submitting Windows credentials through the Credential Provider

That makes input-triggered unlock, background-policy unlock, and presence locking easy to confuse.

## Decision

Use a small two-state authentication orchestrator.

The orchestrator tracks only whether a camera authentication job is currently running:

```text
not_running + trigger request -> start one camera auth job
running     + trigger request -> attach to or ignore the existing job
job ended                   -> return to not_running
```

Do not model `completed`, `failed`, or `cooldown` as orchestrator states. Those are job outcomes
or retry policy details, not the question "is an auth job currently running?"

## Module Boundaries

### Trigger Sources

Trigger sources do not open cameras and do not run face matching. They only ask the service to
start authentication.

Required trigger sources:

- `input-triggered`: user interaction at the lock screen, such as keyboard/mouse/credential-page interaction.
- `background-policy`: background policy deciding that an unlock attempt should be started.
- `hybrid`: product setting that enables both trigger sources.

Trigger source names must be persisted/logged as concrete enum values, not booleans.

### Auth Orchestrator

The orchestrator owns deduplication:

- if no job is running, create one job and return `camera_auth_job_started`
- if a job is already running, do not open another camera; return `camera_auth_job_already_running`
- when the job finishes, clear the running marker

It may temporarily cache a completed authorization grant for Provider pickup, but the cache is not
the orchestrator state.

### Camera Face Auth Job

The job owns the actual authentication work:

- `camera_opened`
- `frame_acquired`
- `liveness_evidence_accepted`
- `face_match_threshold_passed`
- `auth_grant_issued`

Failure outcomes must name the failing condition:

- `camera_open_failed`
- `frame_read_failed`
- `no_face_detected`
- `multiple_faces_detected`
- `liveness_evidence_rejected`
- `face_match_below_threshold`
- `credential_material_fetch_rejected`

Avoid generic `success`, `failure`, `ok`, or bare booleans in protocol/log names.

### Credential Provider

The Provider is a Windows LogonUI adapter:

- receives LogonUI lifecycle calls
- emits input-triggered auth requests
- fetches an issued grant or waits for the current job
- serializes credential material
- reports Windows credential submission result

The Provider does not own background policy and does not decide camera backend selection.

### Background Policy

Background policy is separate from Provider:

- low-frequency presence/unlock policy can emit `background-policy` trigger requests
- it must use the same orchestrator entrypoint as input-triggered unlock
- it must not open a second camera when an input-triggered job is already running

## Runtime Contract

All trigger sources send the same request shape:

```text
auth_trigger_source = input-triggered | background-policy
auth_session_id = credential-provider-logon-auto-wake
auth_source = local-camera
```

The service replies or logs with concrete outcomes such as:

- `camera_auth_job_started`
- `camera_auth_job_already_running`
- `auth_grant_issued`
- `auth_rejected_no_face_detected`
- `auth_rejected_face_match_below_threshold`
- `auth_rejected_liveness_evidence`
- `auth_rejected_internal_error`

Existing protocol names can remain temporarily if changing the wire protocol is too large, but logs
and internal types must use the concrete vocabulary above.

Do not shorten these to `success`, `failure`, `ok`, `completed`, or boolean flags. For example,
`auth_grant_issued` means face authentication produced a grant, while
`credential_material_delivered` means Provider credential material was returned. They are different
layers and must not share one generic "success" word.

## Implementation Notes

- Keep the current named pipe IPC.
- Keep JSON camera backend profiles; camera selection is not Provider logic.
- Keep a single camera auth job per service session.
- Use structured logs to trace:
  `trigger_source -> orchestrator decision -> camera auth job outcome -> provider credential submission`.

## Verification

- Unit test duplicate trigger requests return the same running job outcome.
- Unit test a finished job clears the running marker.
- Manual log check should show one camera auth job for overlapping input/background triggers.
