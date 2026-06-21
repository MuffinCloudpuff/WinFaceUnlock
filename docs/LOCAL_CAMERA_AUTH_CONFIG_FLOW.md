# Local Camera Auth Config Flow

## Problem

The setup program installs the service, Credential Provider, binaries, and model
files. It must not depend on a face template that may not exist yet.

Face enrollment runs later from the control UI. That flow knows the selected
camera and produces the selected template file, so it owns the final local
camera auth configuration.

## Boundary

- Setup writes installation artifacts and static service/provider registration.
- Control UI sends the selected camera in `start_face_enrollment`.
- Control backend finishes enrollment and sends the service the selected
  template path plus the selected camera ID.
- Windows Service writes HKLM service auth config as the privileged component.
- Lock-screen auth reads only service config and opens that configured camera.

## Contract

Enrollment completion applies:

```text
AuthMode = local-camera
FaceTemplatePath = <enrollment selected_templates.json>
CameraId = <camera used during enrollment>
YuNetModelPath = <install dir>/models/face_detection_yunet_2023mar.onnx
SFaceModelPath = <install dir>/models/face_recognition_sface_2021dec.onnx
MiniFasNetModelPath = <install dir>/models/minifasnet_v2.onnx
```

The install dir is derived from the running service executable path, not a
hard-coded drive.

## Verification

After enrollment, `installer_cli service-auth-status` should show
`auth_mode: local-camera` and the selected `camera_id`.
