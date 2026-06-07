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
    EnrollWindowsCredential,
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
    InvalidCredentialEnrollmentRequest,
    DashboardStatusUnavailable,
    SettingsUnavailable,
    SettingsPersistenceFailed,
    CredentialEnrollmentUnavailable,
    CredentialEnrollmentFailed,
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

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct WindowsCredentialEnrollmentOutcome {
    pub windows_account_username: String,
    pub user_id: String,
    pub user_sid: String,
    pub account_type: WindowsCredentialAccountType,
    pub credential_ref: String,
}

fn default_control_user_id() -> String {
    "dev-user".to_owned()
}

fn default_control_user_sid() -> String {
    "S-1-5-21-winfaceunlock-pending".to_owned()
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
}
