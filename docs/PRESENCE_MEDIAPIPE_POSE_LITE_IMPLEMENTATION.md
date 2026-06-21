# Presence Lock: MediaPipe Pose Lite Implementation

## Decision

Presence Lock should use MediaPipe Pose Lite as the primary seated-presence signal.
Face recognition remains the unlock/authentication signal. It must not be the only
signal for deciding whether the user has left the seat.

This fixes the current false-lock class where the user is still in frame, but the
face detector loses the face because the user looks down, turns sideways, or is
partly occluded.

## Current State

The service already has two presence routes:

- `face-owner-match`: uses face recognition as presence.
- `opencv-dnn-person`: uses OpenCV DNN person boxes.

The repository also has a `face_pose_mediapipe` crate and a native MediaPipe
bridge, but that bridge currently wraps **Face Landmarker**. It is used for face
pose/blink during diagnostics enrollment. It is not a body pose detector.

So the new route must share the same bridge pattern, but use a separate Pose
Landmarker ABI, model path, result type, and service config.

## Target Contract

Registry/config values:

```text
PresenceDetectorKind=mediapipe-pose-lite
PresenceTrackingMode=continuous-low-fps
PresencePoseBridgeDllPath=<install_dir>\native\winfaceunlock_mediapipe_bridge.dll
PresencePoseModelPath=<install_dir>\models\pose_landmarker_lite.task
PresencePoseMinLandmarkVisibility=0.45
PresencePoseMinLandmarkPresence=0.45
```

Existing person-policy settings are reused for cadence and absence confirmation:

```text
PresenceDetectorFps
PresencePersonSuspectFps
PresenceAbsentRequiredFrames
PresenceBoundaryMarginRatio
PresenceMovementDeltaRatio
```

The protocol names are intentionally explicit: `PresencePose...` means MediaPipe
body pose, not face pose, not identity auth, and not credential submission.

## Module Boundary

- `face_pose_mediapipe`
  - Owns dynamic loading of `winfaceunlock_mediapipe_bridge.dll`.
  - Adds a Pose Landmarker presence provider next to the existing Face Landmarker
    provider.
  - Returns a simple seated-presence estimate: detected/not detected, confidence,
    normalized box, center, and area.

- `native/mediapipe_bridge`
  - Keeps the existing Face Landmarker ABI.
  - Adds independent `winfaceunlock_mediapipe_presence_pose_*` exports.
  - Uses MediaPipe Pose Landmarker C API and the Lite `.task` model.

- `win_service::presence_mediapipe_pose_detector`
  - Adapts the MediaPipe pose provider to the service-side presence detector
    concept.
  - Contains no camera ownership and no lock policy.

- `win_service::presence_mediapipe_pose_camera`
  - Owns camera open/read.
  - Converts pose estimates to `PresenceObservation::PersonPresent` or
    `PresenceObservation::PersonAbsent`.

- `win_service::presence_service`
  - Chooses the MediaPipe route only when
    `PresenceDetectorKind=mediapipe-pose-lite` and
    `PresenceTrackingMode=continuous-low-fps`.

- `installer_cli` / `setup_api`
  - Accept setup payload fields for bridge/model relative paths.
  - Resolve them under the user-selected install directory.
  - Persist paths into HKLM service config.

## Detection Rule

The pose route does not care whether the face is visible.

For each low-FPS frame:

1. Run Pose Landmarker Lite on the camera frame.
2. Use visible/present body landmarks to build a normalized upper-body box.
3. Prefer nose, shoulders, and hips; shoulders alone are enough for seated
   presence.
4. Emit `PersonPresent` when enough landmarks pass visibility/presence gates.
5. Emit `PersonAbsent` only when the pose route cannot find enough usable body
   landmarks.

The existing presence policy handles debounce, suspect cadence, boundary drift,
and lock decision. This keeps model inference separate from lock policy.

## Packaging Requirement

The code expects these runtime files to be staged under the install directory:

```text
native\winfaceunlock_mediapipe_bridge.dll
models\pose_landmarker_lite.task
```

The current repository contains MediaPipe Pose C headers, but the Lite `.task`
model is not present in `models`. The route can be compiled and configured, but
the installed service cannot use it until the model and bridge DLL are included
in the setup payload.

## Validation

Minimum checks:

```powershell
cargo fmt --check
cargo test -p face_pose_mediapipe -p win_service -p installer_cli -p setup_api --quiet
```

Runtime checks after packaging:

```powershell
installer_cli configure-presence-lock `
  --detector-kind mediapipe-pose-lite `
  --tracking-mode continuous-low-fps `
  --presence-lock-enabled true
```

Then verify:

- Looking down at a phone keeps `PersonPresent`.
- Turning away keeps `PersonPresent` if shoulders/upper body remain visible.
- Standing up and leaving frame eventually locks after the configured absent
  frame count.
