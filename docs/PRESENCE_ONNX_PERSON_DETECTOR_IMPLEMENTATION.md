# Presence ONNX Person Detector Implementation

## Decision

Presence lock uses an ONNX person detector as the product path. The service keeps the existing `PresenceObservationSource` and `PresencePolicy` boundaries; only the detector implementation changes.

MediaPipe Pose Lite is no longer the default implementation because building MediaPipe Tasks C++ on the current Windows/MSVC toolchain required invasive third-party compatibility patches. That work is not proportional to the feature: presence lock only needs a stable "person present / person absent" signal.

## Runtime Shape

- `win_service` owns the presence monitor and policy.
- `presence_person_detector` loads `models/yolov8n.onnx` through ONNX Runtime via the Rust `ort` crate.
- `yolov8-onnx` remains a compatibility value for the older OpenCV DNN executor; new installs use `ort-yolov8-onnx`.
- `presence_person_camera` reads camera frames and converts detections into `PresenceObservation::PersonPresent` or `PersonAbsent`.
- setup writes detector config through registry values:
  - `PresenceDetectorKind=opencv-dnn-person`
  - `PresenceTrackingMode=continuous-low-fps`
  - `PresencePersonDetectorModel=ort-yolov8-onnx`
  - `PresencePersonModelPath=<install-dir>\models\yolov8n.onnx`
  - `PresencePersonModelConfigPath` cleared

## Packaging

The setup payload requires `models/yolov8n.onnx` and `onnxruntime.dll` for presence lock.

The setup payload no longer requires:

- `models/pose_landmarker_lite.task`
- `native/winfaceunlock_mediapipe_bridge.dll`

MediaPipe-related files may remain as experimental source code, but they must not block setup packaging or the default product path.

## Follow-Up

If we later need keypoint-level body pose, use a Windows-friendly ONNX pose model behind the same detector boundary instead of reviving MediaPipe Tasks C++ as a required runtime dependency.
