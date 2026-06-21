# Camera backend profiling

## Problem

`opencv-index:N` only identifies a camera index. OpenCV still needs a Windows capture backend:
`msmf`, `dshow`, or `any`. On real hardware these can differ by tens of seconds. The service
must not discover this during enrollment or unlock.

## Decision

The Windows service owns camera backend profiling.

- On service startup, profile all currently visible cameras in the background.
- On camera device changes, profile again in the background.
- Store the fastest readable backend per camera in an install-local JSON file.
- Enrollment, presence monitoring, and unlock use the stored backend before fallback probing.
- If a stored backend fails, the normal fallback still works and the profiler refreshes later.

## Storage

Path:

`<install-dir>\config\camera_backend_profiles.json`

Shape:

```json
{
  "profiles": [
    {
      "camera_id": "opencv-index:1",
      "display_name": "Logitech HD Pro Webcam C920",
      "preferred_backend": "dshow",
      "open_ms": 640,
      "read_ms": 101,
      "frame_width": 640,
      "frame_height": 480,
      "measured_at_unix_ms": 1781940000000
    }
  ]
}
```

`camera_id` is the initial key because the current provider contract is index-based. `display_name`
is stored as a weak device signature so later device-path support can replace the key without
changing users' saved preference semantics.

## Runtime Flow

1. Service starts.
2. `CameraBackendProfileService` enumerates cameras without opening streams.
3. For each camera, it measures `msmf`, `dshow`, and `any`.
4. It writes the fastest backend that opens and returns a non-empty first frame.
5. `OpenCvCameraProvider` reads `preferred_backend` from config and tries it first.
6. If no profile exists, it uses fallback order.
7. Windows device change notification triggers another profiling pass.

## Observability

Service log records:

- profiling start/completion
- per-camera selected backend and timing
- probe failures
- device-change-triggered refresh

No credentials or frame image data are logged.

## Test Strategy

- Unit test JSON read/write selection.
- Unit test backend ordering uses profile first.
- Manual diagnostic command: `camera-open-benchmark`.
