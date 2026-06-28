use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SETUP_PROTOCOL_VERSION: u32 = 1;
pub const SETUP_PAYLOAD_MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupRequestEnvelope {
    pub protocol_version: u32,
    pub correlation_id: String,
    pub operation: SetupOperation,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupResponseEnvelope {
    pub protocol_version: u32,
    pub correlation_id: String,
    pub operation: SetupOperation,
    pub operation_status: OperationStatus,
    pub message: String,
    #[serde(default)]
    pub safe_details: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_recommended_action: Option<String>,
}

impl SetupResponseEnvelope {
    pub fn succeeded(
        request: &SetupRequestEnvelope,
        message: impl Into<String>,
        safe_details: Value,
    ) -> Self {
        Self {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Succeeded,
            message: message.into(),
            safe_details,
            next_recommended_action: None,
        }
    }

    pub fn failed(
        request: &SetupRequestEnvelope,
        message: impl Into<String>,
        setup_error_code: SetupErrorCode,
    ) -> Self {
        Self {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Failed,
            message: message.into(),
            safe_details: json!({
                "setup_error_code": setup_error_code,
            }),
            next_recommended_action: Some(
                "Review the setup error and retry after remediation.".to_owned(),
            ),
        }
    }

    pub fn invalid_request(
        request: &SetupRequestEnvelope,
        message: impl Into<String>,
        safe_details: Value,
    ) -> Self {
        Self {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::InvalidRequest,
            message: message.into(),
            safe_details,
            next_recommended_action: Some("Correct the setup request payload.".to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupOperation {
    GetStatus,
    RunPreflight,
    InspectPayload,
    StagePayload,
    EnrollCredential,
    EnrollFaceTemplate,
    RunAuthSelfTest,
    InstallSystemComponents,
    ConfigurePresenceLock,
    Repair,
    EmergencyDisable,
    Uninstall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Succeeded,
    Failed,
    RequiresElevation,
    RequiresUserInput,
    BlockedByRunningProcess,
    Cancelled,
    UnsupportedProtocol,
    InvalidRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupErrorCode {
    InvalidInstallDir,
    MissingPayloadFile,
    MissingModelFile,
    StagePayloadFailed,
    CredentialEnrollmentFailed,
    FaceEnrollmentFailed,
    AuthSelfTestFailed,
    ServiceInstallFailed,
    ProviderInstallFailed,
    PresenceConfigFailed,
    RepairFailed,
    EmergencyDisableFailed,
    UninstallFailed,
    InsufficientPrivileges,
    ExternalHostUnavailable,
    InvalidRequest,
    UnsupportedOperation,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct UninstallPayload {
    #[serde(default)]
    pub preserve_data: bool,
    #[serde(default = "default_stop_service_first")]
    pub stop_service_first: bool,
}

impl Default for UninstallPayload {
    fn default() -> Self {
        Self {
            preserve_data: false,
            stop_service_first: true,
        }
    }
}

fn default_stop_service_first() -> bool {
    true
}

fn default_payload_manifest_version() -> u32 {
    SETUP_PAYLOAD_MANIFEST_VERSION
}

fn default_payload_manifest_relative_path() -> PathBuf {
    PathBuf::from("winfaceunlock-payload.json")
}

fn default_payload_file_required() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PreflightPayload {
    pub install_dir: PathBuf,
    #[serde(default)]
    pub require_elevation: bool,
    #[serde(default)]
    pub required_payload_files: Vec<RequiredPayloadFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct InspectPayloadPayload {
    pub payload_root_dir: PathBuf,
    #[serde(default = "default_payload_manifest_relative_path")]
    pub manifest_relative_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RequiredPayloadFile {
    pub file_id: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupPayloadManifest {
    #[serde(default = "default_payload_manifest_version")]
    pub manifest_version: u32,
    #[serde(default)]
    pub payload_files: Vec<SetupPayloadManifestFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupPayloadManifestFile {
    pub file_id: String,
    pub source_relative_path: PathBuf,
    pub target_relative_path: Option<PathBuf>,
    #[serde(default = "default_payload_file_required")]
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct StagePayloadPayload {
    pub install_dir: PathBuf,
    pub payload_root_dir: Option<PathBuf>,
    #[serde(default)]
    pub overwrite_existing: bool,
    #[serde(default)]
    pub payload_files: Vec<StagePayloadFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct StagePayloadFile {
    pub file_id: String,
    pub source_path: PathBuf,
    pub target_relative_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct EnrollCredentialPayload {
    pub install_dir: Option<PathBuf>,
    pub username: String,
    #[serde(default = "default_user_id")]
    pub user_id: String,
    #[serde(default = "default_user_sid")]
    pub user_sid: String,
    #[serde(default = "default_account_type")]
    pub account_type: String,
    pub credential_ref: Option<String>,
    pub store_dir: Option<PathBuf>,
    pub password_secret_transport: Option<CredentialSecretTransportPayload>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CredentialSecretTransportPayload {
    pub transport_kind: String,
    pub pipe_name: String,
    pub secret_nonce: String,
    #[serde(default = "default_secret_transport_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct EnrollFaceTemplatePayload {
    pub install_dir: PathBuf,
    #[serde(default = "default_camera_id")]
    pub camera_id: String,
    #[serde(default = "default_user_id")]
    pub user_id: String,
    #[serde(default = "default_guided_enrollment_output_relative_dir")]
    pub output_relative_dir: PathBuf,
    #[serde(default = "default_face_template_relative_path")]
    pub output_template_relative_path: PathBuf,
    #[serde(default = "default_guided_enrollment_accepted_frames_per_step")]
    pub accepted_frames_per_step: u32,
    #[serde(default = "default_guided_enrollment_max_wait_frames_per_step")]
    pub max_wait_frames_per_step: u32,
    #[serde(default = "default_guided_enrollment_max_frames_per_step")]
    pub max_frames_per_step: u32,
    #[serde(default = "default_guided_enrollment_pose_ready_consecutive")]
    pub pose_ready_consecutive: u32,
    #[serde(default = "default_guided_enrollment_pose_ready_min_fit")]
    pub pose_ready_min_fit: f32,
    #[serde(default = "default_guided_enrollment_frame_delay_ms")]
    pub frame_delay_ms: u32,
    #[serde(default)]
    pub allow_partial_enrollment: bool,
    #[serde(default)]
    pub save_debug_images: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RunAuthSelfTestPayload {
    pub install_dir: PathBuf,
    #[serde(default = "default_auth_self_test_session_id")]
    pub session_id: String,
    #[serde(default = "default_require_credential_ready")]
    pub require_credential_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct InstallSystemComponentsPayload {
    pub install_dir: PathBuf,
    #[serde(default = "default_start_service")]
    pub start_service: bool,
    #[serde(default)]
    pub configure_local_camera_auth: bool,
    #[serde(default = "default_service_binary_relative_path")]
    pub service_binary_relative_path: PathBuf,
    #[serde(default = "default_control_tray_relative_path")]
    pub control_tray_relative_path: PathBuf,
    #[serde(default = "default_provider_binary_relative_path")]
    pub provider_binary_relative_path: PathBuf,
    #[serde(default = "default_face_template_relative_path")]
    pub face_template_relative_path: PathBuf,
    #[serde(default = "default_yunet_model_relative_path")]
    pub yunet_model_relative_path: PathBuf,
    #[serde(default = "default_sface_model_relative_path")]
    pub sface_model_relative_path: PathBuf,
    #[serde(default = "default_minifasnet_model_relative_path")]
    pub minifasnet_model_relative_path: PathBuf,
    #[serde(default = "default_presence_person_model_relative_path")]
    pub presence_person_model_relative_path: PathBuf,
    #[serde(default = "default_presence_person_model_config_relative_path")]
    pub presence_person_model_config_relative_path: PathBuf,
    #[serde(default = "default_presence_pose_bridge_relative_path")]
    pub presence_pose_bridge_relative_path: PathBuf,
    #[serde(default = "default_presence_pose_model_relative_path")]
    pub presence_pose_model_relative_path: PathBuf,
    #[serde(default = "default_camera_id")]
    pub camera_id: String,
    pub match_threshold: Option<f32>,
    pub required_consecutive_match_count: Option<u32>,
    #[serde(default)]
    pub provider_mode: ProviderModePayload,
}

impl Default for InstallSystemComponentsPayload {
    fn default() -> Self {
        Self {
            install_dir: PathBuf::new(),
            start_service: default_start_service(),
            configure_local_camera_auth: false,
            service_binary_relative_path: default_service_binary_relative_path(),
            control_tray_relative_path: default_control_tray_relative_path(),
            provider_binary_relative_path: default_provider_binary_relative_path(),
            face_template_relative_path: default_face_template_relative_path(),
            yunet_model_relative_path: default_yunet_model_relative_path(),
            sface_model_relative_path: default_sface_model_relative_path(),
            minifasnet_model_relative_path: default_minifasnet_model_relative_path(),
            presence_person_model_relative_path: default_presence_person_model_relative_path(),
            presence_person_model_config_relative_path:
                default_presence_person_model_config_relative_path(),
            presence_pose_bridge_relative_path: default_presence_pose_bridge_relative_path(),
            presence_pose_model_relative_path: default_presence_pose_model_relative_path(),
            camera_id: default_camera_id(),
            match_threshold: None,
            required_consecutive_match_count: None,
            provider_mode: ProviderModePayload::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProviderModePayload {
    #[serde(default = "default_wake_auth_source")]
    pub wake_auth_source: String,
    #[serde(default = "default_tile_visibility")]
    pub tile_visibility: String,
    #[serde(default = "default_auto_wake_on_advise")]
    pub auto_wake_on_advise: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ConfigurePresenceLockPayload {
    pub install_dir: Option<PathBuf>,
    pub presence_lock_enabled: Option<bool>,
    pub presence_owner_match_threshold: Option<f32>,
    pub detector_kind: Option<String>,
    pub tracking_mode: Option<String>,
    pub detector_fps: Option<f32>,
    pub unload_model_when_idle: Option<bool>,
    pub person_confidence_threshold: Option<f32>,
    pub person_detector_model: Option<String>,
    pub person_suspect_fps: Option<f32>,
    pub absent_required_frames: Option<u32>,
    pub boundary_margin_ratio: Option<f32>,
    pub movement_delta_ratio: Option<f32>,
    pub person_model_relative_path: Option<PathBuf>,
    pub person_model_config_relative_path: Option<PathBuf>,
    #[serde(default)]
    pub clear_person_model_config: bool,
    pub person_debug_output_dir: Option<PathBuf>,
    #[serde(default)]
    pub clear_person_debug_output_dir: bool,
    pub pose_bridge_relative_path: Option<PathBuf>,
    pub pose_model_relative_path: Option<PathBuf>,
    pub pose_min_landmark_visibility: Option<f32>,
    pub pose_min_landmark_presence: Option<f32>,
}

impl Default for ProviderModePayload {
    fn default() -> Self {
        Self {
            wake_auth_source: default_wake_auth_source(),
            tile_visibility: default_tile_visibility(),
            auto_wake_on_advise: default_auto_wake_on_advise(),
        }
    }
}

fn default_start_service() -> bool {
    true
}

fn default_user_id() -> String {
    "dev-user".to_owned()
}

fn default_user_sid() -> String {
    "S-1-5-21-winfaceunlock-pending".to_owned()
}

fn default_account_type() -> String {
    "local".to_owned()
}

fn default_guided_enrollment_output_relative_dir() -> PathBuf {
    PathBuf::from("face-enrollment")
}

fn default_guided_enrollment_accepted_frames_per_step() -> u32 {
    6
}

fn default_guided_enrollment_max_wait_frames_per_step() -> u32 {
    180
}

fn default_guided_enrollment_max_frames_per_step() -> u32 {
    180
}

fn default_guided_enrollment_pose_ready_consecutive() -> u32 {
    3
}

fn default_guided_enrollment_pose_ready_min_fit() -> f32 {
    0.25
}

fn default_guided_enrollment_frame_delay_ms() -> u32 {
    60
}

fn default_auth_self_test_session_id() -> String {
    "setup-auth-self-test".to_owned()
}

fn default_secret_transport_timeout_ms() -> u64 {
    30_000
}

fn default_require_credential_ready() -> bool {
    true
}

fn default_auto_wake_on_advise() -> bool {
    true
}

fn default_camera_id() -> String {
    "opencv-index:0".to_owned()
}

fn default_wake_auth_source() -> String {
    "local-camera".to_owned()
}

fn default_tile_visibility() -> String {
    "hidden-until-ready".to_owned()
}

fn default_service_binary_relative_path() -> PathBuf {
    PathBuf::from("win_service.exe")
}

fn default_control_tray_relative_path() -> PathBuf {
    PathBuf::from("control_tray.exe")
}

fn default_provider_binary_relative_path() -> PathBuf {
    PathBuf::from(r"provider\windows_provider.dll")
}

fn default_face_template_relative_path() -> PathBuf {
    PathBuf::from("selected_templates.json")
}

fn default_yunet_model_relative_path() -> PathBuf {
    PathBuf::from(r"models\face_detection_yunet_2023mar.onnx")
}

fn default_sface_model_relative_path() -> PathBuf {
    PathBuf::from(r"models\face_recognition_sface_2021dec.onnx")
}

fn default_minifasnet_model_relative_path() -> PathBuf {
    PathBuf::from(r"models\minifasnet_v2.onnx")
}

fn default_presence_person_model_relative_path() -> PathBuf {
    PathBuf::from(r"models\yolov8n.onnx")
}

fn default_presence_person_model_config_relative_path() -> PathBuf {
    PathBuf::new()
}

fn default_presence_pose_bridge_relative_path() -> PathBuf {
    PathBuf::from(r"native\winfaceunlock_mediapipe_bridge.dll")
}

fn default_presence_pose_model_relative_path() -> PathBuf {
    PathBuf::from(r"models\pose_landmarker_lite.task")
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupPreflightCheck {
    pub check_id: String,
    pub status: SetupStepStatus,
    pub message: String,
}

impl SetupPreflightCheck {
    pub fn succeeded(check_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            check_id: check_id.into(),
            status: SetupStepStatus::Succeeded,
            message: message.into(),
        }
    }

    pub fn failed(check_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            check_id: check_id.into(),
            status: SetupStepStatus::Failed,
            message: message.into(),
        }
    }

    pub fn skipped(check_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            check_id: check_id.into(),
            status: SetupStepStatus::Skipped,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_envelope_round_trips_snake_case_operation() -> Result<(), serde_json::Error> {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-1".to_owned(),
            operation: SetupOperation::EmergencyDisable,
            payload: json!({}),
        };

        let encoded = serde_json::to_string(&request)?;
        assert!(encoded.contains("\"operation\":\"emergency_disable\""));

        let decoded: SetupRequestEnvelope = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn uninstall_payload_defaults_to_full_uninstall() -> Result<(), serde_json::Error> {
        let payload: UninstallPayload = serde_json::from_value(json!({}))?;

        assert!(!payload.preserve_data);
        assert!(payload.stop_service_first);
        Ok(())
    }

    #[test]
    fn preflight_payload_defaults_to_no_elevation_and_no_payload_files()
    -> Result<(), serde_json::Error> {
        let payload: PreflightPayload = serde_json::from_value(json!({
            "install_dir": "C:\\WinFaceUnlock"
        }))?;

        assert!(!payload.require_elevation);
        assert!(payload.required_payload_files.is_empty());
        Ok(())
    }

    #[test]
    fn inspect_payload_defaults_to_standard_manifest_name() -> Result<(), serde_json::Error> {
        let payload: InspectPayloadPayload = serde_json::from_value(json!({
            "payload_root_dir": "C:\\Users\\Leo\\Downloads\\WinFaceUnlockPayload"
        }))?;

        assert_eq!(
            payload.manifest_relative_path,
            PathBuf::from("winfaceunlock-payload.json")
        );
        Ok(())
    }

    #[test]
    fn payload_manifest_files_default_to_required() -> Result<(), serde_json::Error> {
        let manifest: SetupPayloadManifest = serde_json::from_value(json!({
            "payload_files": [
                {
                    "file_id": "win_service",
                    "source_relative_path": "win_service.exe",
                    "target_relative_path": null
                }
            ]
        }))?;

        assert_eq!(manifest.manifest_version, SETUP_PAYLOAD_MANIFEST_VERSION);
        assert!(manifest.payload_files[0].required);
        assert_eq!(manifest.payload_files[0].target_relative_path, None);
        Ok(())
    }

    #[test]
    fn stage_payload_defaults_to_no_overwrite_and_no_payload_files() -> Result<(), serde_json::Error>
    {
        let payload: StagePayloadPayload = serde_json::from_value(json!({
            "install_dir": "C:\\WinFaceUnlock"
        }))?;

        assert_eq!(payload.payload_root_dir, None);
        assert!(!payload.overwrite_existing);
        assert!(payload.payload_files.is_empty());
        Ok(())
    }

    #[test]
    fn credential_payload_defaults_to_local_dev_user() -> Result<(), serde_json::Error> {
        let payload: EnrollCredentialPayload = serde_json::from_value(json!({
            "username": "Leo16"
        }))?;

        assert_eq!(payload.user_id, "dev-user");
        assert_eq!(payload.user_sid, "S-1-5-21-winfaceunlock-pending");
        assert_eq!(payload.account_type, "local");
        assert_eq!(payload.install_dir, None);
        assert_eq!(payload.credential_ref, None);
        assert_eq!(payload.store_dir, None);
        assert_eq!(payload.password_secret_transport, None);
        Ok(())
    }

    #[test]
    fn credential_payload_accepts_named_pipe_secret_transport() -> Result<(), serde_json::Error> {
        let payload: EnrollCredentialPayload = serde_json::from_value(json!({
            "username": "Leo16",
            "password_secret_transport": {
                "transport_kind": "windows_named_pipe_utf8_v1",
                "pipe_name": "winfaceunlock-credential-abc",
                "secret_nonce": "nonce-1"
            }
        }))?;

        assert_eq!(
            payload
                .password_secret_transport
                .as_ref()
                .map(|transport| transport.transport_kind.as_str()),
            Some("windows_named_pipe_utf8_v1")
        );
        assert_eq!(
            payload
                .password_secret_transport
                .as_ref()
                .map(|transport| transport.pipe_name.as_str()),
            Some("winfaceunlock-credential-abc")
        );
        assert_eq!(
            payload
                .password_secret_transport
                .as_ref()
                .map(|transport| transport.secret_nonce.as_str()),
            Some("nonce-1")
        );
        assert_eq!(
            payload
                .password_secret_transport
                .as_ref()
                .map(|transport| transport.timeout_ms),
            Some(30_000)
        );
        Ok(())
    }

    #[test]
    fn face_enrollment_payload_defaults_to_packaged_relative_paths() -> Result<(), serde_json::Error>
    {
        let payload: EnrollFaceTemplatePayload = serde_json::from_value(json!({
            "install_dir": "D:\\Apps\\WinFaceUnlock"
        }))?;

        assert_eq!(payload.camera_id, "opencv-index:0");
        assert_eq!(payload.user_id, "dev-user");
        assert_eq!(
            payload.output_relative_dir,
            PathBuf::from("face-enrollment")
        );
        assert_eq!(
            payload.output_template_relative_path,
            PathBuf::from("selected_templates.json")
        );
        assert_eq!(payload.accepted_frames_per_step, 6);
        assert_eq!(payload.max_wait_frames_per_step, 180);
        assert!(!payload.allow_partial_enrollment);
        Ok(())
    }

    #[test]
    fn auth_self_test_payload_defaults_to_credential_ready_requirement()
    -> Result<(), serde_json::Error> {
        let payload: RunAuthSelfTestPayload = serde_json::from_value(json!({
            "install_dir": "D:\\Apps\\WinFaceUnlock"
        }))?;

        assert_eq!(payload.session_id, "setup-auth-self-test");
        assert!(payload.require_credential_ready);
        Ok(())
    }

    #[test]
    fn install_system_components_defaults_to_relative_packaged_paths()
    -> Result<(), serde_json::Error> {
        let payload: InstallSystemComponentsPayload = serde_json::from_value(json!({
            "install_dir": "D:\\Apps\\WinFaceUnlock"
        }))?;

        assert!(payload.start_service);
        assert!(!payload.configure_local_camera_auth);
        assert_eq!(
            payload.service_binary_relative_path,
            PathBuf::from("win_service.exe")
        );
        assert_eq!(
            payload.control_tray_relative_path,
            PathBuf::from("control_tray.exe")
        );
        assert_eq!(
            payload.provider_binary_relative_path,
            PathBuf::from(r"provider\windows_provider.dll")
        );
        assert_eq!(
            payload.yunet_model_relative_path,
            PathBuf::from(r"models\face_detection_yunet_2023mar.onnx")
        );
        assert_eq!(payload.provider_mode, ProviderModePayload::default());
        Ok(())
    }

    #[test]
    fn configure_presence_lock_payload_accepts_partial_updates() -> Result<(), serde_json::Error> {
        let payload: ConfigurePresenceLockPayload = serde_json::from_value(json!({
            "presence_lock_enabled": true,
            "detector_kind": "opencv-dnn-person"
        }))?;

        assert_eq!(payload.presence_lock_enabled, Some(true));
        assert_eq!(payload.detector_kind.as_deref(), Some("opencv-dnn-person"));
        assert!(!payload.clear_person_model_config);
        Ok(())
    }
}
