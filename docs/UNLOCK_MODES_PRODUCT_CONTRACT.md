# Unlock Modes Product Contract

This document records the agreed product semantics for WinFaceUnlock unlock behavior.
It is the source of truth for future implementation and UI naming.

## Final User-Facing Modes

WinFaceUnlock keeps only two unlock modes:

1. Triggered Recognition
2. Background Silent Recognition

Do not expose a third "hybrid" or "smart mixed" mode. Mixing background and triggered
behavior under one vague name makes the product hard to reason about and hard to debug.

## Shared Security Audit

Security audit is shared by both modes. It is not owned by Background Silent Recognition.

When either mode observes suspicious authentication activity, the audit layer may record
evidence according to the configured audit policy. Suspicious activity includes:

- liveness or anti-spoof rejection
- a visible face whose similarity is below the trusted-user threshold
- repeated failed recognition attempts during a lock-screen session
- other future intrusion signals defined by the face-auth policy

Audit records must be separated from unlock decisions:

- The unlock mode decides when recognition runs.
- The face-auth policy decides whether the current frame can unlock.
- The audit policy decides whether failed or suspicious observations are persisted.

Audit records should avoid storing unnecessary sensitive data. If face snapshots are
stored, the retention policy, storage location, encryption requirements, and user-facing
controls must be explicit before the feature is enabled by default.

## Triggered Recognition

Triggered Recognition is event-driven.

Expected user flow:

1. Windows is locked and shows the lock-screen cover.
2. The user clicks, presses a key, or otherwise moves from the cover screen into the
   credential/PIN/password input screen.
3. WinFaceUnlock starts one recognition round.
4. If the face is live and matches a trusted user, WinFaceUnlock unlocks immediately.
5. If recognition fails, the round stops and the camera is released.
6. If Windows later returns to the lock-screen cover, and the user again moves into the
   credential/PIN/password input screen, WinFaceUnlock starts a new recognition round.

Important constraints:

- Do not start recognition merely because the machine entered the locked state.
- Do not continuously hold the camera in this mode.
- A failed round should not permanently suppress the next user-triggered round.
- The trigger is the lock-screen-cover to credential-screen transition, not a generic
  background timer.
- The maximum frame count is a failure boundary, not a success delay. Once a valid
  live match is accepted, authorization should be published immediately.

## Background Silent Recognition

Background Silent Recognition is continuous lock-screen monitoring.

Expected user flow:

1. Windows enters the locked state.
2. WinFaceUnlock owns the camera continuously while the system remains in the applicable
   lock/logon monitoring state.
3. Recognition runs continuously, not as a low-frequency periodic check.
4. If the face is live and matches a trusted user, WinFaceUnlock unlocks immediately.
5. If suspicious activity is observed, the shared security audit layer may record it.
6. When the system unlocks or the mode is disabled, camera ownership stops.

Important constraints:

- This mode is allowed to keep the camera active for the whole lock-screen period.
- The UI must communicate that this mode continuously uses the camera while locked.
- Camera ownership, resource usage, privacy expectations, and audit retention must be
  observable and configurable.

## Authorization Timing

For both modes, the unlock decision should not wait for the whole recognition window.

The correct timing model is:

1. Frames are evaluated until a terminal result is produced.
2. When live-face matching passes policy, publish the authorization result immediately.
3. Stop or wind down camera/model resources after the terminal decision.
4. Resource cleanup must not be on the critical path before the credential provider can
   receive the successful authorization, except where required by a proven security
   invariant.

This means a 30-frame window, if configured, is the maximum failure window. It must not
mean "always wait for 30 frames before unlocking."

## Naming Guidance

Preferred internal names:

- `TriggeredRecognition`
- `BackgroundSilentRecognition`
- `SecurityAuditPolicy`
- `RecognitionRound`
- `UnlockAuthorizationIssued`

Avoid ambiguous names:

- `hybrid`
- `smart`
- `background-policy`
- `auto-wake`
- generic `success`, `ok`, or `flag` fields for cross-module unlock state

Cross-process or persisted state should use structured enums and explicit event names,
for example:

- `UnlockMode::TriggeredRecognition`
- `UnlockMode::BackgroundSilentRecognition`
- `AuthTriggerSource::CredentialScreenEntered`
- `AuthTriggerSource::BackgroundSilentMonitor`
- `AuditReason::LivenessRejected`
- `AuditReason::SimilarityBelowThreshold`

## Implementation Direction

Future implementation should converge on these boundaries:

- Credential Provider detects or approximates the credential-screen entry event for
  Triggered Recognition.
- Windows Service owns camera access, face recognition, liveness, authorization, and
  audit decisions.
- IPC carries explicit trigger source, unlock mode, recognition status, failure reason,
  and audit metadata.
- The control UI only configures mode and audit policy; it should not encode service
  business logic.

Existing legacy terms such as `background-policy`, `input-triggered`, `hybrid`, and
`AutoWakeOnAdvise` should be migrated or wrapped behind the new terminology instead of
being exposed as product concepts.

## Current Implementation Notes

The current implementation maps the product contract as follows:

- `triggered-recognition` starts recognition from the Credential Provider after the
  credential screen is entered or the WinFaceUnlock credential is selected.
- `background-silent-recognition` continuously retries recognition while the Credential
  Provider remains advised by LogonUI.
- Legacy persisted values are accepted for compatibility:
  `input-triggered` maps to `triggered-recognition`; `background-policy` and `hybrid`
  map to `background-silent-recognition`.
- New writes must persist only `triggered-recognition` or
  `background-silent-recognition`.

The service-level version of Background Silent Recognition should eventually start from
Windows lock/session-change events and keep a service-owned recognition monitor alive
before LogonUI loads the Credential Provider. That needs an explicit service-to-provider
authorization handoff so the Provider can consume a background-issued grant without
starting a duplicate camera job.
