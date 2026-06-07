# Setup Backend API Contract

## Purpose

This document defines the reusable setup backend contract for WinFaceUnlock.

It must be callable by:

1. the standalone WinUI 3 setup application
2. an external Windows Fluent frontend
3. automated VM validation scripts
4. command-line recovery workflows

The frontend owns presentation. The setup backend owns install semantics,
privileged operations, state validation, and safe error reporting.

This document does not define the post-install runtime control panel protocol.
Runtime control frontends, including Tauri and any future WinUI 3 control
frontend, must share a frontend-independent control contract. See
`docs/CONTROL_FRONTEND_PROTOCOL_ARCHITECTURE.md`.

## Design Rules

1. All operations use structured request and response objects.
2. Every operation carries a `correlation_id`.
3. Responses use explicit semantic status values, not ambiguous `ok` booleans.
4. Passwords and plaintext credential material must never appear in logs,
   process arguments, JSON responses, or temp files.
5. Operations that mutate system state must be idempotent where practical.
6. Provider registration is always last during install and first during
   uninstall.
7. Full uninstall is the default. Data retention requires explicit
   `preserve_data`.

## Transport Phases

### Phase 8 MVP

The first transport is process execution with JSON input/output:

```text
setup_backend command -> JSON request on stdin or file handle
setup_backend response <- JSON response on stdout
```

Implementation may initially be added to `installer_cli` as `--json` commands,
but the contract should be named `setup_backend` in code and tests so it can
move to a dedicated executable or named-pipe server later.

Current implementation status:

```text
Implemented:
  installer_cli setup-backend
  get_status
  run_preflight
  inspect_payload
  stage_payload
  enroll_credential via WinUI secret pipe with native prompt fallback
  enroll_face_template
  run_auth_self_test
  install_system_components
  configure_presence_lock
  repair
  emergency_disable
  uninstall
```

### Later

If UI progress streaming or cancellation becomes important, add:

```text
\\.\pipe\WinFaceUnlock.SetupBackend.v1
```

The operation schema remains the same.

## Common Types

### SetupRequestEnvelope

```json
{
  "protocol_version": 1,
  "correlation_id": "phase8-...",
  "operation": "get_status",
  "payload": {}
}
```

### SetupResponseEnvelope

```json
{
  "protocol_version": 1,
  "correlation_id": "phase8-...",
  "operation": "get_status",
  "operation_status": "succeeded",
  "message": "Status loaded.",
  "safe_details": {},
  "next_recommended_action": null
}
```

### OperationStatus

```text
succeeded
failed
requires_elevation
requires_user_input
blocked_by_running_process
cancelled
unsupported_protocol
invalid_request
```

### SetupStepStatus

```text
pending
running
succeeded
failed
skipped
```

### SetupErrorCode

```text
invalid_install_dir
missing_payload_file
missing_model_file
stage_payload_failed
credential_enrollment_failed
face_enrollment_failed
auth_self_test_failed
service_install_failed
provider_install_failed
presence_config_failed
repair_failed
emergency_disable_failed
uninstall_failed
insufficient_privileges
external_host_unavailable
invalid_request
unsupported_operation
```

## Operations

## get_status

Reads current install state without mutating the system.

Request:

```json
{
  "operation": "get_status",
  "payload": {}
}
```

Response `safe_details`:

```json
{
  "install_dir": "C:\\WinFaceUnlock",
  "service": {
    "installed": true,
    "running": true,
    "binary_path": "C:\\WinFaceUnlock\\win_service.exe"
  },
  "provider": {
    "credential_provider_registered": true,
    "com_server_registered": true,
    "project_config_registered": true,
    "dll_path": "C:\\WinFaceUnlock\\windows_provider.dll"
  },
  "service_config": {
    "auth_mode": "local-camera",
    "presence_lock_enabled": false,
    "presence_detector_kind": "face-owner-match"
  },
  "data": {
    "program_data_exists": true,
    "credential_store_exists": true
  }
}
```

## run_preflight

Validates install directory, payload files, elevation, and recovery readiness.

Request:

```json
{
  "operation": "run_preflight",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "require_elevation": true,
    "required_payload_files": [
      {
        "file_id": "win_service",
        "path": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload\\win_service.exe"
      },
      {
        "file_id": "windows_provider",
        "path": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload\\windows_provider.dll"
      }
    ]
  }
}
```

Response `safe_details`:

```json
{
  "checks": [
    {
      "check_id": "install_dir_writable",
      "status": "succeeded",
      "message": "Install directory is writable."
    },
    {
      "check_id": "payload_complete",
      "status": "succeeded",
      "message": "Required payload files are present."
    }
  ]
}
```

If `required_payload_files` is omitted or empty, the `payload_complete` check is
reported as `skipped`. If `require_elevation` is true and the process is not
elevated, the response `operation_status` is `requires_elevation`.

## inspect_payload

Reads `winfaceunlock-payload.json` from an extracted payload directory and
returns payload file readiness plus the file list that can be passed to
`stage_payload`.

Request:

```json
{
  "operation": "inspect_payload",
  "payload": {
    "payload_root_dir": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload",
    "manifest_relative_path": "winfaceunlock-payload.json"
  }
}
```

Payload manifest:

```json
{
  "manifest_version": 1,
  "payload_files": [
    {
      "file_id": "win_service",
      "source_relative_path": "win_service.exe",
      "target_relative_path": "win_service.exe",
      "required": true
    },
    {
      "file_id": "yolox_nano_model",
      "source_relative_path": "models\\yolox_nano.onnx",
      "target_relative_path": "models\\yolox_nano.onnx",
      "required": false
    }
  ]
}
```

If `target_relative_path` is omitted, it defaults to `source_relative_path`.
If `required` is omitted, it defaults to `true`.

Response `safe_details`:

```json
{
  "payload_root_dir": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload",
  "manifest_path": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload\\winfaceunlock-payload.json",
  "inspected_files": [
    {
      "file_id": "win_service",
      "source_relative_path": "win_service.exe",
      "target_relative_path": "win_service.exe",
      "source_path": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload\\win_service.exe",
      "required": true,
      "payload_file_presence_status": "present"
    }
  ],
  "missing_required_payload_files": [],
  "stage_payload_files": [
    {
      "file_id": "win_service",
      "source_path": "win_service.exe",
      "target_relative_path": "win_service.exe"
    }
  ]
}
```

`payload_file_presence_status` values:

```text
present
missing_required
missing_optional
```

Rules:

1. `payload_root_dir` must be an existing absolute directory.
2. `manifest_relative_path`, `source_relative_path`, and
   `target_relative_path` must be relative and must not contain parent path
   traversal.
3. Missing required files return `operation_status: failed` with
   `setup_error_code: missing_payload_file`.
4. Missing optional files are reported but omitted from `stage_payload_files`.

## stage_payload

Copies setup payload into the selected install directory.

Request:

```json
{
  "operation": "stage_payload",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "payload_root_dir": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload",
    "overwrite_existing": true,
    "payload_files": [
      {
        "file_id": "win_service",
        "source_path": "win_service.exe",
        "target_relative_path": "win_service.exe"
      },
      {
        "file_id": "windows_provider",
        "source_path": "windows_provider.dll",
        "target_relative_path": "windows_provider.dll"
      },
      {
        "file_id": "yunet_model",
        "source_path": "models\\face_detection_yunet_2023mar.onnx",
        "target_relative_path": "models\\face_detection_yunet_2023mar.onnx"
      }
    ]
  }
}
```

Response `safe_details`:

```json
{
  "install_dir": "C:\\WinFaceUnlock",
  "staged_files": [
    {
      "file_id": "win_service",
      "source_path": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload\\win_service.exe",
      "target_path": "C:\\WinFaceUnlock\\win_service.exe",
      "stage_file_status": "copied"
    }
  ]
}
```

`stage_file_status` values:

```text
copied
replaced
skipped_existing_identical
```

Rules:

1. Do not overwrite a loaded Provider DLL in place.
2. Prefer versioned payload names or stop/restart flows where needed.
3. Ensure recovery scripts are copied before Provider registration.
4. `target_relative_path` must be relative and must not contain parent path
   traversal.
5. `source_path` may be absolute, but packaged installers should prefer a
   relative `source_path` plus `payload_root_dir` so extracted payloads work
   from any download or temp directory.
6. Relative `source_path` values are resolved under `payload_root_dir` and must
   not contain parent path traversal.

## enroll_credential

Stores the Windows credential in the encrypted credential store.

Request shape:

```json
{
  "operation": "enroll_credential",
  "payload": {
    "username": "Leo16",
    "user_id": "dev-user",
    "user_sid": "S-1-5-21-winfaceunlock-pending",
    "account_type": "local",
    "credential_ref": "windows-credential-dev-user",
    "store_dir": "C:\\ProgramData\\WinFaceUnlock",
    "password_secret_transport": {
      "transport_kind": "windows_named_pipe_utf8_v1",
      "pipe_name": "winfaceunlock-credential-0123456789abcdef",
      "secret_nonce": "0123456789abcdef0123456789abcdef",
      "timeout_ms": 30000
    }
  }
}
```

The contract intentionally does not define a plaintext password field. The
WinUI installer creates a one-shot named pipe, passes only the pipe descriptor
and nonce in JSON, and writes the password bytes through that pipe. The backend
validates the descriptor, verifies the nonce, then writes the returned password
directly into the encrypted credential store. If `password_secret_transport` is
omitted, `installer_cli setup-backend` falls back to the native Windows
credential prompt. Unit tests do not open the native prompt.

Allowed implementations:

1. one-shot local secret pipe with nonce validation
2. native credential prompt fallback
3. in-process secure buffer passed directly to Rust code
4. future OS credential broker integration

Disallowed implementations:

1. command-line password argument
2. JSON plaintext password
3. temp file password handoff
4. logging password length or content

## enroll_face_template

Captures and stores selected face templates.

Request:

```json
{
  "operation": "enroll_face_template",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "camera_id": "opencv-index:0",
    "user_id": "dev-user",
    "output_relative_dir": "face-enrollment",
    "output_template_relative_path": "selected_templates.json",
    "accepted_frames_per_step": 6,
    "max_wait_frames_per_step": 180,
    "max_frames_per_step": 180,
    "pose_ready_consecutive": 3,
    "pose_ready_min_fit": 0.25,
    "frame_delay_ms": 60,
    "allow_partial_enrollment": false,
    "save_debug_images": false
  }
}
```

Response `safe_details`:

```json
{
  "install_dir": "C:\\WinFaceUnlock",
  "camera_id": "opencv-index:0",
  "user_id": "dev-user",
  "diagnostics_cli_path": "C:\\WinFaceUnlock\\diagnostics_cli.exe",
  "enrollment_output_dir": "C:\\WinFaceUnlock\\face-enrollment",
  "generated_template_path": "C:\\WinFaceUnlock\\face-enrollment\\selected_templates.json",
  "template_path": "C:\\WinFaceUnlock\\selected_templates.json",
  "template_summary": {
    "selected_template_count": 5,
    "rejected_sample_count": 0
  }
}
```

Implementation note: `diagnostics_cli.exe` is resolved under `install_dir`, and
model paths remain relative to that install directory.

## run_auth_self_test

Runs a local camera auth test after the Service has been installed and started.

Request:

```json
{
  "operation": "run_auth_self_test",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "session_id": "phase8-self-test",
    "require_credential_ready": true
  }
}
```

Response `safe_details`:

```json
{
  "install_dir": "C:\\WinFaceUnlock",
  "diagnostics_cli_path": "C:\\WinFaceUnlock\\diagnostics_cli.exe",
  "session_id": "phase8-self-test",
  "auth_succeeded": true,
  "credential_ready": true,
  "require_credential_ready": true,
  "exit_code": 0
}
```

`auth_succeeded = false` or `credential_ready = false` should block final
Provider validation unless the user explicitly chooses to continue in a debug
or recovery flow.

## install_system_components

Installs the Windows Service and Credential Provider. By default this operation
only installs system components; it does not configure local-camera
authentication, does not require a face template, and does not enroll Windows
credentials. The post-install configuration panel owns those steps.

Request:

```json
{
  "operation": "install_system_components",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "start_service": true,
    "configure_local_camera_auth": false,
    "service_binary_relative_path": "win_service.exe",
    "provider_binary_relative_path": "windows_provider.dll",
    "provider_mode": {
      "wake_auth_source": "local-camera",
      "tile_visibility": "hidden-until-ready",
      "auto_wake_on_advise": true
    }
  }
}
```

Path rules:

1. Request paths for staged binaries are relative to `install_dir`.
2. If `configure_local_camera_auth` is false, model and template paths are not
   validated and no local-camera auth registry config is written.
3. If `configure_local_camera_auth` is true, model and template paths are
   relative to `install_dir`, validated, and resolved before writing Windows
   Service auth registry configuration.
3. This supports installs outside `C:\`, for example
   `D:\Apps\WinFaceUnlock\models\...`.
4. Relative paths must not contain parent traversal.

Rules:

1. Configure Service auth only when `configure_local_camera_auth` is true.
2. Install or repair the Service before Provider registration.
3. Register Provider last.
3. Restart Service when install or repair updates the service binary path.

## configure_presence_lock

Updates only presence lock settings.

Request:

```json
{
  "operation": "configure_presence_lock",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "presence_lock_enabled": true,
    "detector_kind": "opencv-dnn-person",
    "tracking_mode": "continuous-low-fps",
    "detector_fps": 2.0,
    "unload_model_when_idle": false,
    "person_detector_model": "yolov8-onnx"
  }
}
```

Path rules:

1. `install_dir` is optional for simple switch and threshold updates.
2. `install_dir` is required when setting `person_detector_model`,
   `person_model_relative_path`, or `person_model_config_relative_path`.
3. Presence model paths are relative to `install_dir` and are resolved to
   absolute paths before writing Service registry configuration.
4. `person_detector_model: "mobilenet-ssd"` defaults to
   `models\MobileNetSSD_deploy.caffemodel` and
   `models\MobileNetSSD_deploy.prototxt`.
5. `person_detector_model: "yolov8-onnx"` defaults to
   `models\yolov8n.onnx` and clears the model config path.

Rules:

1. Do not touch face template or auth settings.
2. Presence lock remains disabled by default.

## repair

Restores Service, Provider, ACLs, and recovery scripts.

Request:

```json
{
  "operation": "repair",
  "payload": {
    "install_dir": "C:\\WinFaceUnlock",
    "start_service": true,
    "service_binary_relative_path": "win_service.exe",
    "provider_binary_relative_path": "windows_provider.dll",
    "provider_mode": {
      "wake_auth_source": "local-camera",
      "tile_visibility": "hidden-until-ready",
      "auto_wake_on_advise": true
    }
  }
}
```

Path rules match `install_system_components`: staged binaries are expressed as
paths relative to `install_dir`. Local-camera auth paths are considered only
when `configure_local_camera_auth` is true.

Rules:

1. Recreate missing registry keys.
2. Repair service config.
3. Restart service if requested.
4. Re-register Provider after Service repair.

## emergency_disable

Disables only the Credential Provider enumeration entry.

Request:

```json
{
  "operation": "emergency_disable",
  "payload": {}
}
```

Rules:

1. Do not delete credential store data.
2. Do not delete Service.
3. Do not delete COM/project config unless full uninstall is requested.

## uninstall

Performs full uninstall by default.

Request:

```json
{
  "operation": "uninstall",
  "payload": {
    "preserve_data": false,
    "stop_service_first": true
  }
}
```

Rules:

1. Remove Provider first.
2. Stop and delete Service.
3. Delete `C:\ProgramData\WinFaceUnlock` unless `preserve_data` is true.
4. Leave the external host project untouched.

## Frontend Host Discovery API

This protocol is separate from setup backend operations.

Pipe:

```text
\\.\pipe\WinFaceUnlock.UiHost.v1
```

Request:

```json
{
  "type": "open_setup_page",
  "protocol_version": 1,
  "correlation_id": "...",
  "mode": "install",
  "suggested_install_dir": "C:\\WinFaceUnlock"
}
```

Response:

```json
{
  "type": "open_setup_page_result",
  "protocol_version": 1,
  "correlation_id": "...",
  "opened": true
}
```

If this request fails quickly, WinFaceUnlock starts its standalone WinUI setup
app.

## Validation Requirements

Before real-machine use:

1. JSON schema tests for all operation requests and responses.
2. VM setup flow using standalone WinUI app.
3. VM setup flow using a mock external host.
4. VM emergency-disable and repair flow.
5. VM full uninstall flow verifying ProgramData deletion.
6. Full workspace checks:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
```
