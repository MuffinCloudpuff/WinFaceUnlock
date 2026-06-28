# Presence Lock Final Sampling Policy

## Scope

This document defines the production Presence Lock sampling policy for the desktop
unlocked state. It is the source of truth for how human input, camera sampling,
person detection, and lock requests interact.

Presence Lock only runs after the user has logged into the desktop. It never
participates in LogonUI authentication, never issues `AuthGrant`, and never uses
its low-confidence presence results to unlock the machine.

## State Ownership

`camera_runtime`
  Owns interface state and camera leases. `PresenceLock` leases are allowed only
  while the interface state is `DesktopUnlocked`.

`presence_sampling_gate`
  Owns the decision to open the camera for this round. The first production
  signal is keyboard/mouse quiet duration from the desktop input agent.

`presence_person_camera`
  Owns a short camera sampling window. It opens the camera, skips transient bad
  frames inside the window, emits one structured observation, closes the camera,
  and releases the lease.

`presence_policy`
  Owns the lock state machine: stable person present, absence confirmation, and
  lock request.

## Human Input Gate

The service must not open the camera while recent keyboard or mouse input is
present.

```text
human_input_quiet_threshold_ms = 60_000
gate_recheck_interval_ms = 60_000
```

Rules:

```text
human_input_quiet_duration_ms < 60_000
=> skip camera sampling
=> recheck human input state later

human_input_quiet_duration_ms >= 60_000
=> allow one short camera sampling window
```

Keyboard/mouse input is not a permanent proof that the owner is present. It is
only a low-cost reason to avoid opening the camera while the user is visibly
interacting with the machine.

## Short Camera Sampling Window

Each allowed sampling round must be bounded:

1. Acquire `CameraLeaseKind::PresenceLock`.
2. Open the configured camera using the profiled backend.
3. Read frames until the first valid detection input is available, with bounded
   tolerance for transient open-time failures.
4. Emit exactly one observation.
5. Close the camera and release the lease.

These transient frame failures must not count as "person absent":

```text
EmptyFrame
ReadFailed
InvalidFrame
```

If the window cannot obtain a valid detection result after its bounded retries,
the observation is `CameraUnavailable`. `CameraUnavailable` never requests a
lock by itself.

## Person Detection Policy

The default production path is person detection:

```text
presence_detector_kind = opencv-dnn-person
presence_tracking_mode = continuous-low-fps
```

Despite the historical `continuous-low-fps` name, the camera is not held open
continuously. The policy is interval-based and every interval uses a short
camera sampling window.

### Stable Person Present

After the first valid observation detects a person:

```text
1st person-present observation => next check after 10 seconds
2nd consecutive person-present observation => next check after 30 seconds
3rd and later consecutive person-present observations => next check after 60 seconds
```

This is the stable backoff path. It prevents the service from repeatedly
opening the camera at high frequency while someone is clearly still in front of
the computer.

Any later `PersonAbsent` observation leaves stable-present mode and enters
absence confirmation.

### Absence Confirmation

After the first valid observation detects no person:

```text
absence_confirm_interval_ms = 1_000
absence_required_valid_observations = 3
```

Rules:

```text
PersonAbsent #1 => suspect, check again after 1 second
PersonAbsent #2 => still suspect, check again after 1 second
PersonAbsent #3 => request lock immediately
```

If any confirmation observation detects a person, absence confirmation is reset
and the policy returns to stable-present backoff.

Only valid `PersonAbsent` observations increment the absence counter. Camera
open failures and invalid frames do not.

## Face Policy Compatibility

The older face-policy path keeps its existing owner/no-face/unknown-face
semantics. It remains useful for diagnostics and future identity-aware presence
work, but the default production Presence Lock path should use person detection
with the stable-present backoff described above.

## Observability

Logs must make each round explainable:

```text
PresenceSamplingGate.SkipSampling reason=recent-human-input ...
PresenceSamplingGate.AllowSampling reason=human-input-quiet ...
PresencePersonSource.SampleStarted ...
PresencePersonSource.SampleCompleted observation=...
PresencePersonSource.SampleReleased
PresenceMonitor.Stopped summary=...
```

The important distinction is:

```text
PersonAbsent => valid absence evidence
CameraUnavailable => infrastructure failure, no lock evidence
```

## Acceptance Criteria

1. Recent keyboard/mouse input prevents camera sampling.
2. After 60 seconds without keyboard/mouse input, the service opens one short
   sampling window.
3. The first valid person-present observation schedules the next check in
   10 seconds, then 30 seconds, then 60 seconds.
4. The first valid person-absent observation enters 1 second confirmation.
5. Three consecutive valid person-absent observations request lock.
6. Invalid frames, empty frames, read failures, and camera-open failures do not
   count as absence.
7. Every sampling window releases the camera before the next policy wait.
