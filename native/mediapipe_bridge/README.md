# WinFaceUnlock MediaPipe Bridge

This directory contains the native adapter used by `face_pose_mediapipe`.

It is intentionally separate from the Rust login service:

- `win_service` does not link this DLL.
- default `diagnostics_cli` does not link this DLL.
- only `diagnostics_cli --features mediapipe-pose` can load it.

## ABI

The exported ABI is declared in:

```text
include/winfaceunlock_mediapipe_bridge.h
```

The Rust-side contract is documented in:

```text
docs/MEDIAPIPE_BRIDGE_ABI.md
```

## Inputs

The bridge wraps MediaPipe Face Landmarker C API:

- `MpFaceLandmarkerCreate`
- `MpFaceLandmarkerDetectImage`
- `MpFaceLandmarkerCloseResult`
- `MpFaceLandmarkerClose`
- `MpImageCreateFromUint8Data`
- `MpImageFree`

Official reference:

- https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker
- https://github.com/google-ai-edge/mediapipe/tree/master/mediapipe/tasks/c/vision/face_landmarker

## Build Shape

This CMake project expects a MediaPipe C Tasks include root and import/static library:

```powershell
cmake -S native\mediapipe_bridge -B target\mediapipe_bridge `
  -G Ninja `
  -DMEDIAPIPE_INCLUDE_DIR=<path-to-mediapipe-repo-or-include-root> `
  -DMEDIAPIPE_TASKS_C_LIB=<path-to-mediapipe-tasks-c-lib>

cmake --build target\mediapipe_bridge
```

Equivalent project helper:

```powershell
.\scripts\build_mediapipe_bridge.ps1 `
  -MediaPipeIncludeDir <path-to-mediapipe-repo-or-include-root> `
  -MediaPipeTasksCLib <path-to-mediapipe-tasks-c-lib>
```

Copy the resulting DLL to:

```text
native/winfaceunlock_mediapipe_bridge.dll
```

Then build the diagnostics CLI with:

```powershell
cargo build -p diagnostics_cli --features mediapipe-pose
```

## Current Status

This is the project-local bridge contract and implementation. The remaining
external task is producing a Windows MediaPipe Tasks C library with MSVC/Bazel
or a compatible prebuilt artifact.
