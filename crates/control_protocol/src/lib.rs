use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CONTROL_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ControlRequestEnvelope {
    pub protocol_version: u32,
    pub correlation_id: String,
    pub operation: ControlOperation,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ControlResponseEnvelope {
    pub protocol_version: u32,
    pub correlation_id: String,
    pub operation: ControlOperation,
    pub operation_status: ControlOperationStatus,
    pub message: String,
    #[serde(default)]
    pub safe_details: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_recommended_action: Option<String>,
}

impl ControlResponseEnvelope {
    pub fn completed(
        request: &ControlRequestEnvelope,
        message: impl Into<String>,
        safe_details: Value,
    ) -> Self {
        Self {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: ControlOperationStatus::Completed,
            message: message.into(),
            safe_details,
            next_recommended_action: None,
        }
    }

    pub fn failed(
        request: &ControlRequestEnvelope,
        message: impl Into<String>,
        control_error_code: ControlErrorCode,
    ) -> Self {
        Self {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: ControlOperationStatus::Failed,
            message: message.into(),
            safe_details: json!({
                "control_error_code": control_error_code,
            }),
            next_recommended_action: Some(
                "Review the control error and retry after remediation.".to_owned(),
            ),
        }
    }

    pub fn invalid_request(
        request: &ControlRequestEnvelope,
        message: impl Into<String>,
        safe_details: Value,
    ) -> Self {
        Self {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: ControlOperationStatus::InvalidRequest,
            message: message.into(),
            safe_details,
            next_recommended_action: Some("Correct the control request payload.".to_owned()),
        }
    }

    pub fn unsupported_protocol(request: &ControlRequestEnvelope) -> Self {
        Self {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: ControlOperationStatus::UnsupportedProtocol,
            message: format!(
                "Unsupported control protocol version {}.",
                request.protocol_version
            ),
            safe_details: json!({
                "supported_protocol_version": CONTROL_PROTOCOL_VERSION,
            }),
            next_recommended_action: Some("Update the control frontend or backend.".to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlOperation {
    GetDashboardStatus,
    GetSettings,
    UpdateSettings,
    GetWindowsCredentialAccount,
    EnrollWindowsCredential,
    ListFaceTemplates,
    DeleteFaceTemplate,
    ListCameras,
    StartFaceEnrollment,
    GetFaceEnrollmentStatus,
    GetFaceEnrollmentPreview,
    CancelFaceEnrollment,
    FinishFaceEnrollment,
    RunFaceAuthSelfTest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlOperationStatus {
    Completed,
    Failed,
    RequiresElevation,
    RequiresUserInput,
    ServiceUnavailable,
    PermissionDenied,
    InvalidRequest,
    UnsupportedProtocol,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    InvalidDashboardStatusRequest,
    InvalidSettingsRequest,
    InvalidCredentialAccountRequest,
    InvalidCredentialEnrollmentRequest,
    InvalidFaceTemplateRequest,
    InvalidFaceEnrollmentRequest,
    InvalidAuthSelfTestRequest,
    DashboardStatusUnavailable,
    SettingsUnavailable,
    SettingsPersistenceFailed,
    CredentialAccountUnavailable,
    CredentialEnrollmentUnavailable,
    CredentialEnrollmentFailed,
    FaceTemplateStoreUnavailable,
    FaceTemplateConfigMissing,
    FaceTemplateFileMissing,
    FaceTemplateParseFailed,
    FaceTemplateEmpty,
    FaceTemplateNotFound,
    ActiveTemplateDeleteBlocked,
    FaceTemplateDeleteFailed,
    FaceEnrollmentUnavailable,
    FaceEnrollmentAlreadyRunning,
    FaceEnrollmentSessionNotFound,
    CameraUnavailable,
    FaceModelUnavailable,
    FaceEnrollmentFailed,
    FaceEnrollmentCancelled,
    FaceTemplateMissing,
    CredentialMissing,
    AuthMatchFailed,
    GrantIssueFailed,
    CredentialMaterialUnavailable,
    AuthSelfTestFailed,
    ServiceStatusUnavailable,
    ProviderStatusUnavailable,
    ServiceConfigUnavailable,
    DataDirectoryStatusUnavailable,
    PresenceRuntimeStatusUnavailable,
    ElevationRequired,
    PermissionDenied,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ControlSettingsSnapshot {
    pub presence_lock_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logon_wake_mode: Option<LogonWakeMode>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct ControlSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_lock_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logon_wake_mode: Option<LogonWakeMode>,
}

impl ControlSettingsPatch {
    pub fn has_updates(&self) -> bool {
        self.presence_lock_enabled.is_some() || self.logon_wake_mode.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogonWakeMode {
    InputTriggered,
    BackgroundPolicy,
    Hybrid,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct WindowsCredentialEnrollmentPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_account_username: Option<String>,
    #[serde(default = "default_control_user_id")]
    pub user_id: String,
    #[serde(default = "default_control_user_sid")]
    pub user_sid: String,
    #[serde(default)]
    pub account_type: WindowsCredentialAccountType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct WindowsCredentialAccountProfile {
    pub windows_account_username: String,
    pub user_id: String,
    pub user_sid: String,
    pub account_type: WindowsCredentialAccountType,
    pub credential_ref: String,
    pub credential_secret_state: WindowsCredentialSecretState,
}

impl WindowsCredentialEnrollmentPayload {
    pub fn has_valid_safe_fields(&self) -> bool {
        !self.user_id.trim().is_empty()
            && !self.user_sid.trim().is_empty()
            && self
                .credential_ref
                .as_deref()
                .is_none_or(|credential_ref| !credential_ref.trim().is_empty())
            && self
                .windows_account_username
                .as_deref()
                .is_none_or(|username| !username.trim().is_empty())
    }

    pub fn resolved_credential_ref(&self) -> String {
        self.credential_ref
            .clone()
            .unwrap_or_else(|| format!("windows-credential-{}", self.user_id))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsCredentialAccountType {
    #[default]
    Local,
    MicrosoftAccount,
    Domain,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsCredentialSecretState {
    Configured,
    #[default]
    NotConfigured,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct WindowsCredentialEnrollmentOutcome {
    pub windows_account_username: String,
    pub user_id: String,
    pub user_sid: String,
    pub account_type: WindowsCredentialAccountType,
    pub credential_ref: String,
    pub credential_secret_state: WindowsCredentialSecretState,
}

fn default_control_user_id() -> String {
    "dev-user".to_owned()
}

fn default_control_user_sid() -> String {
    "S-1-5-21-winfaceunlock-pending".to_owned()
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceTemplateList {
    pub templates: Vec<FaceTemplateSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceTemplateSummary {
    pub face_template_ref: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub template_kind: FaceTemplateKind,
    pub recognition_model: FaceRecognitionModelSummary,
    pub selected_template_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_sample_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_unix_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_unix_ms: Option<i64>,
    pub source_state: FaceTemplateSourceState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceTemplateKind {
    SelectedTemplateSet,
    RepositoryTemplate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceTemplateSourceState {
    ActiveServiceTemplate,
    RepositoryTemplate,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceRecognitionModelSummary {
    pub model_family: String,
    pub model_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DeleteFaceTemplatePayload {
    pub face_template_ref: String,
}

impl DeleteFaceTemplatePayload {
    pub fn has_valid_ref(&self) -> bool {
        !self.face_template_ref.trim().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DeleteFaceTemplateOutcome {
    pub face_template_ref: String,
    pub template_deleted: bool,
    pub service_auth_requires_reconfiguration: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct CameraDeviceList {
    pub cameras: Vec<CameraDeviceSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CameraDeviceSummary {
    pub camera_id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceEnrollmentStartPayload {
    #[serde(default = "default_control_user_id")]
    pub user_id: String,
    #[serde(default = "default_camera_id")]
    pub camera_id: String,
    #[serde(default)]
    pub enrollment_profile: FaceEnrollmentProfile,
    #[serde(default)]
    pub allow_partial_enrollment: bool,
}

impl FaceEnrollmentStartPayload {
    pub fn has_valid_fields(&self) -> bool {
        !self.user_id.trim().is_empty() && !self.camera_id.trim().is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceEnrollmentProfile {
    #[default]
    GuidedStandard,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceEnrollmentSessionPayload {
    pub enrollment_session_id: String,
}

impl FaceEnrollmentSessionPayload {
    pub fn has_valid_session_id(&self) -> bool {
        !self.enrollment_session_id.trim().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceEnrollmentSessionStatus {
    pub enrollment_session_id: String,
    pub session_state: FaceEnrollmentSessionState,
    pub user_id: String,
    pub camera_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_instruction_code: Option<String>,
    pub accepted_sample_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_sample_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_frame_result: Option<FaceEnrollmentFrameResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_summary: Option<FaceTemplateEnrollmentSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceEnrollmentPreviewFrame {
    pub enrollment_session_id: String,
    pub preview_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_updated_at_unix_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceEnrollmentSessionState {
    Starting,
    Running,
    WaitingForFace,
    WaitingForPose,
    Capturing,
    Finishing,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceEnrollmentFrameResult {
    FaceAccepted,
    NoFaceDetected,
    MultipleFacesDetected,
    PoseNotReady,
    QualityRejected,
    ModelUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceTemplateEnrollmentSummary {
    pub selected_template_count: u32,
    pub rejected_sample_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceEnrollmentFinishOutcome {
    pub enrollment_session_id: String,
    pub session_state: FaceEnrollmentSessionState,
    pub face_template_ref: String,
    pub user_id: String,
    pub template_summary: FaceTemplateEnrollmentSummary,
    pub service_auth_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_auth_configuration_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FaceAuthSelfTestPayload {
    #[serde(default = "default_face_auth_self_test_session_id")]
    pub session_id: String,
    #[serde(default = "default_require_credential_ready")]
    pub require_credential_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_id: Option<String>,
}

impl FaceAuthSelfTestPayload {
    pub fn has_valid_fields(&self) -> bool {
        !self.session_id.trim().is_empty()
            && self
                .camera_id
                .as_deref()
                .is_none_or(|camera_id| !camera_id.trim().is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct FaceAuthSelfTestOutcome {
    pub session_id: String,
    pub auth_match_passed: bool,
    pub grant_issued: bool,
    pub credential_material_ready: bool,
    pub credential_decryption_succeeded: bool,
    pub pipe_delivery_confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_match_score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_face_template_ref: Option<String>,
}

fn default_camera_id() -> String {
    "opencv-index:0".to_owned()
}

fn default_face_auth_self_test_session_id() -> String {
    "control-auth-self-test".to_owned()
}

fn default_require_credential_ready() -> bool {
    true
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DashboardStatus {
    pub service: ServiceStatusSummary,
    pub provider: ProviderStatusSummary,
    pub service_config: ServiceConfigSummary,
    pub data_directory: DataDirectorySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_runtime: Option<PresenceRuntimeSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ServiceStatusSummary {
    pub installation_state: ServiceInstallationState,
    pub runtime_state: ServiceRuntimeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceInstallationState {
    Installed,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceRuntimeState {
    Running,
    Stopped,
    Paused,
    StartPending,
    StopPending,
    Missing,
    Unknown(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProviderStatusSummary {
    pub registration_state: ProviderRegistrationState,
    pub credential_provider_registered: bool,
    pub com_server_registered: bool,
    pub project_config_registered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRegistrationState {
    Registered,
    PartiallyRegistered,
    NotRegistered,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ServiceConfigSummary {
    pub registry_config_state: RegistryConfigState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_template_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_lock_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_detector_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_tracking_mode: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryConfigState {
    Present,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DataDirectorySummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program_data_dir: Option<String>,
    pub program_data_presence: PathPresence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_audit_dir: Option<String>,
    pub presence_audit_presence: PathPresence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathPresence {
    Present,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct PresenceRuntimeSummary {
    pub monitor_state: PresenceMonitorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_unix_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceMonitorState {
    Running,
    Stopped,
    Disabled,
    Unavailable,
    Unknown(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dashboard_request() -> ControlRequestEnvelope {
        ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-test-1".to_owned(),
            operation: ControlOperation::GetDashboardStatus,
            payload: json!({}),
        }
    }

    fn dashboard_status_fixture() -> DashboardStatus {
        DashboardStatus {
            service: ServiceStatusSummary {
                installation_state: ServiceInstallationState::Installed,
                runtime_state: ServiceRuntimeState::Running,
                process_id: Some(42),
            },
            provider: ProviderStatusSummary {
                registration_state: ProviderRegistrationState::Registered,
                credential_provider_registered: true,
                com_server_registered: true,
                project_config_registered: true,
            },
            service_config: ServiceConfigSummary {
                registry_config_state: RegistryConfigState::Present,
                auth_mode: Some("local_camera".to_owned()),
                face_template_path: Some("models\\face-template.json".to_owned()),
                presence_lock_enabled: Some(true),
                presence_detector_kind: Some("person".to_owned()),
                presence_tracking_mode: Some("owner_face".to_owned()),
            },
            data_directory: DataDirectorySummary {
                program_data_dir: Some("C:\\ProgramData\\WinFaceUnlock".to_owned()),
                program_data_presence: PathPresence::Present,
                presence_audit_dir: Some(
                    "C:\\ProgramData\\WinFaceUnlock\\presence-audit".to_owned(),
                ),
                presence_audit_presence: PathPresence::Present,
            },
            presence_runtime: Some(PresenceRuntimeSummary {
                monitor_state: PresenceMonitorState::Running,
                session_id: Some(1),
                reason: Some("monitor_running".to_owned()),
                updated_at_unix_ms: Some(1_782_000_000_000),
            }),
        }
    }

    #[test]
    fn request_envelope_round_trips_snake_case_operation() -> Result<(), serde_json::Error> {
        let request = dashboard_request();

        let json_text = serde_json::to_string(&request)?;
        assert!(json_text.contains("\"operation\":\"get_dashboard_status\""));

        let decoded: ControlRequestEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn response_envelope_round_trips_dashboard_status() -> Result<(), serde_json::Error> {
        let request = dashboard_request();
        let status = dashboard_status_fixture();
        let safe_details = serde_json::to_value(&status)?;
        let response =
            ControlResponseEnvelope::completed(&request, "Dashboard status loaded.", safe_details);

        let json_text = serde_json::to_string(&response)?;
        assert!(json_text.contains("\"operation_status\":\"completed\""));
        assert!(json_text.contains("\"runtime_state\":\"running\""));
        assert!(!json_text.contains("password"));

        let decoded: ControlResponseEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, response);
        Ok(())
    }

    #[test]
    fn unsupported_protocol_response_names_supported_version() {
        let mut request = dashboard_request();
        request.protocol_version = CONTROL_PROTOCOL_VERSION + 1;

        let response = ControlResponseEnvelope::unsupported_protocol(&request);

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::UnsupportedProtocol
        );
        assert_eq!(
            response.safe_details["supported_protocol_version"],
            CONTROL_PROTOCOL_VERSION
        );
    }

    #[test]
    fn invalid_request_response_uses_semantic_status() {
        let request = dashboard_request();

        let response = ControlResponseEnvelope::invalid_request(
            &request,
            "Dashboard payload must be empty.",
            json!({
                "control_error_code": ControlErrorCode::InvalidDashboardStatusRequest,
            }),
        );

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_dashboard_status_request"
        );
    }

    #[test]
    fn dashboard_status_shape_keeps_service_and_provider_states_distinct() {
        let status = dashboard_status_fixture();

        assert_eq!(
            status.service.installation_state,
            ServiceInstallationState::Installed
        );
        assert_eq!(status.service.runtime_state, ServiceRuntimeState::Running);
        assert_eq!(
            status.provider.registration_state,
            ProviderRegistrationState::Registered
        );
        assert_eq!(
            status.service_config.registry_config_state,
            RegistryConfigState::Present
        );
    }

    #[test]
    fn settings_request_round_trips_snake_case_operation() -> Result<(), serde_json::Error> {
        let request = ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-settings-test-1".to_owned(),
            operation: ControlOperation::UpdateSettings,
            payload: json!(ControlSettingsPatch {
                presence_lock_enabled: Some(true),
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            }),
        };

        let json_text = serde_json::to_string(&request)?;
        assert!(json_text.contains("\"operation\":\"update_settings\""));
        assert!(json_text.contains("\"presence_lock_enabled\":true"));
        assert!(json_text.contains("\"logon_wake_mode\":\"input_triggered\""));

        let decoded: ControlRequestEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn settings_patch_distinguishes_empty_patch_from_false_update() {
        assert!(!ControlSettingsPatch::default().has_updates());
        assert!(
            ControlSettingsPatch {
                presence_lock_enabled: Some(false),
                logon_wake_mode: None,
            }
            .has_updates()
        );
        assert!(
            ControlSettingsPatch {
                presence_lock_enabled: None,
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            }
            .has_updates()
        );
    }

    #[test]
    fn credential_enrollment_request_round_trips_without_password() -> Result<(), serde_json::Error>
    {
        let request = ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-credential-test-1".to_owned(),
            operation: ControlOperation::EnrollWindowsCredential,
            payload: json!(WindowsCredentialEnrollmentPayload {
                windows_account_username: Some("Leo16".to_owned()),
                user_id: "dev-user".to_owned(),
                user_sid: "S-1-5-21-winfaceunlock-pending".to_owned(),
                account_type: WindowsCredentialAccountType::Local,
                credential_ref: None,
            }),
        };

        let json_text = serde_json::to_string(&request)?;
        assert!(json_text.contains("\"operation\":\"enroll_windows_credential\""));
        assert!(json_text.contains("\"windows_account_username\":\"Leo16\""));
        assert!(!json_text.contains("password"));

        let decoded: ControlRequestEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn credential_account_request_round_trips_snake_case_operation() -> Result<(), serde_json::Error>
    {
        let request = ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-credential-account-test-1".to_owned(),
            operation: ControlOperation::GetWindowsCredentialAccount,
            payload: json!({}),
        };

        let json_text = serde_json::to_string(&request)?;
        assert!(json_text.contains("\"operation\":\"get_windows_credential_account\""));

        let decoded: ControlRequestEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn credential_account_profile_round_trips_without_password() -> Result<(), serde_json::Error> {
        let profile = WindowsCredentialAccountProfile {
            windows_account_username: "Leo16".to_owned(),
            user_id: "dev-user".to_owned(),
            user_sid: "S-1-5-21-real".to_owned(),
            account_type: WindowsCredentialAccountType::Local,
            credential_ref: "windows-credential-dev-user".to_owned(),
            credential_secret_state: WindowsCredentialSecretState::Configured,
        };

        let json_text = serde_json::to_string(&profile)?;
        assert!(json_text.contains("\"windows_account_username\":\"Leo16\""));
        assert!(json_text.contains("\"credential_ref\":\"windows-credential-dev-user\""));
        assert!(json_text.contains("\"credential_secret_state\":\"configured\""));
        assert!(!json_text.contains("password"));

        let decoded: WindowsCredentialAccountProfile = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, profile);
        Ok(())
    }

    #[test]
    fn credential_enrollment_payload_defaults_to_runtime_dev_user() -> Result<(), serde_json::Error>
    {
        let payload: WindowsCredentialEnrollmentPayload = serde_json::from_value(json!({}))?;

        assert_eq!(payload.windows_account_username, None);
        assert_eq!(payload.user_id, "dev-user");
        assert_eq!(payload.user_sid, "S-1-5-21-winfaceunlock-pending");
        assert_eq!(payload.account_type, WindowsCredentialAccountType::Local);
        assert_eq!(
            payload.resolved_credential_ref(),
            "windows-credential-dev-user"
        );
        assert!(payload.has_valid_safe_fields());
        Ok(())
    }

    #[test]
    fn face_template_list_request_round_trips_snake_case_operation() -> Result<(), serde_json::Error>
    {
        let request = ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-face-list-test-1".to_owned(),
            operation: ControlOperation::ListFaceTemplates,
            payload: json!({}),
        };

        let json_text = serde_json::to_string(&request)?;
        assert!(json_text.contains("\"operation\":\"list_face_templates\""));

        let decoded: ControlRequestEnvelope = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, request);
        Ok(())
    }

    #[test]
    fn face_template_summary_round_trips_without_template_material() -> Result<(), serde_json::Error>
    {
        let template_list = FaceTemplateList {
            templates: vec![FaceTemplateSummary {
                face_template_ref: "active-service-template".to_owned(),
                user_id: "dev-user".to_owned(),
                display_name: Some("Leo16".to_owned()),
                template_kind: FaceTemplateKind::SelectedTemplateSet,
                recognition_model: FaceRecognitionModelSummary {
                    model_family: "opencv_sface".to_owned(),
                    model_version: "2021dec".to_owned(),
                },
                selected_template_count: 5,
                rejected_sample_count: Some(1),
                created_at_unix_ms: Some(1_782_000_000_000),
                updated_at_unix_ms: Some(1_782_000_000_500),
                source_state: FaceTemplateSourceState::ActiveServiceTemplate,
            }],
        };

        let json_text = serde_json::to_string(&template_list)?;
        assert!(json_text.contains("\"template_kind\":\"selected_template_set\""));
        assert!(json_text.contains("\"source_state\":\"active_service_template\""));
        assert!(!json_text.contains("embedding"));
        assert!(!json_text.contains("image"));
        assert!(!json_text.contains("password"));

        let decoded: FaceTemplateList = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, template_list);
        Ok(())
    }

    #[test]
    fn face_enrollment_start_payload_defaults_to_guided_standard() -> Result<(), serde_json::Error>
    {
        let payload: FaceEnrollmentStartPayload = serde_json::from_value(json!({}))?;

        assert_eq!(payload.user_id, "dev-user");
        assert_eq!(payload.camera_id, "opencv-index:0");
        assert_eq!(
            payload.enrollment_profile,
            FaceEnrollmentProfile::GuidedStandard
        );
        assert!(!payload.allow_partial_enrollment);
        assert!(payload.has_valid_fields());
        Ok(())
    }

    #[test]
    fn face_enrollment_status_round_trips_structured_state() -> Result<(), serde_json::Error> {
        let status = FaceEnrollmentSessionStatus {
            enrollment_session_id: "face-enrollment-1".to_owned(),
            session_state: FaceEnrollmentSessionState::WaitingForPose,
            user_id: "dev-user".to_owned(),
            camera_id: "opencv-index:0".to_owned(),
            current_step: Some("frontal".to_owned()),
            current_instruction_code: Some("look_at_camera".to_owned()),
            accepted_sample_count: 3,
            required_sample_count: Some(6),
            last_frame_result: Some(FaceEnrollmentFrameResult::PoseNotReady),
            template_summary: None,
        };

        let json_text = serde_json::to_string(&status)?;
        assert!(json_text.contains("\"session_state\":\"waiting_for_pose\""));
        assert!(json_text.contains("\"last_frame_result\":\"pose_not_ready\""));

        let decoded: FaceEnrollmentSessionStatus = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, status);
        Ok(())
    }

    #[test]
    fn face_enrollment_finish_outcome_round_trips_service_config_boundary()
    -> Result<(), serde_json::Error> {
        let outcome = FaceEnrollmentFinishOutcome {
            enrollment_session_id: "face-enrollment-1".to_owned(),
            session_state: FaceEnrollmentSessionState::Completed,
            face_template_ref: "face-enrollment-template-face-enrollment-1".to_owned(),
            user_id: "dev-user".to_owned(),
            template_summary: FaceTemplateEnrollmentSummary {
                selected_template_count: 2,
                rejected_sample_count: 1,
            },
            service_auth_configured: false,
            service_auth_configuration_error: Some(
                "service face template configuration update requires elevation".to_owned(),
            ),
        };

        let json_text = serde_json::to_string(&outcome)?;
        assert!(json_text.contains("\"session_state\":\"completed\""));
        assert!(json_text.contains("\"service_auth_configured\":false"));
        assert!(json_text.contains("service_auth_configuration_error"));
        assert!(!json_text.contains("\"success\""));
        assert!(!json_text.contains("selected_templates"));
        assert!(!json_text.contains("embedding"));

        let decoded: FaceEnrollmentFinishOutcome = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, outcome);
        Ok(())
    }

    #[test]
    fn face_auth_self_test_outcome_keeps_auth_layers_distinct() -> Result<(), serde_json::Error> {
        let outcome = FaceAuthSelfTestOutcome {
            session_id: "control-auth-self-test-1".to_owned(),
            auth_match_passed: true,
            grant_issued: true,
            credential_material_ready: true,
            credential_decryption_succeeded: true,
            pipe_delivery_confirmed: false,
            best_match_score: Some(0.81),
            matched_face_template_ref: Some("active-service-template".to_owned()),
        };

        let json_text = serde_json::to_string(&outcome)?;
        assert!(json_text.contains("\"auth_match_passed\":true"));
        assert!(json_text.contains("\"grant_issued\":true"));
        assert!(json_text.contains("\"pipe_delivery_confirmed\":false"));
        assert!(!json_text.contains("\"success\""));

        let decoded: FaceAuthSelfTestOutcome = serde_json::from_str(&json_text)?;
        assert_eq!(decoded, outcome);
        Ok(())
    }
}
