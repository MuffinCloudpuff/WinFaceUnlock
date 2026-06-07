use control_protocol::{
    CONTROL_PROTOCOL_VERSION, ControlErrorCode, ControlOperation, ControlOperationStatus,
    ControlRequestEnvelope, ControlResponseEnvelope, ControlSettingsPatch, ControlSettingsSnapshot,
    DashboardStatus, WindowsCredentialEnrollmentOutcome, WindowsCredentialEnrollmentPayload,
};
use control_status::{
    ControlStatusError, WindowsControlSettingsStore, WindowsDashboardStatusProvider,
};
use serde_json::{Value, json};

pub trait DashboardStatusProvider {
    fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError>;
}

pub trait ControlSettingsStore {
    fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError>;

    fn update_settings(
        &self,
        patch: &ControlSettingsPatch,
    ) -> Result<ControlSettingsSnapshot, ControlBackendError>;
}

pub trait WindowsCredentialEnrollmentStore {
    fn enroll_windows_credential(
        &self,
        payload: &WindowsCredentialEnrollmentPayload,
        password_secret: WindowsCredentialSecret,
    ) -> Result<WindowsCredentialEnrollmentOutcome, ControlBackendError>;
}

#[derive(Eq, PartialEq)]
pub struct WindowsCredentialSecret {
    password: String,
}

impl WindowsCredentialSecret {
    pub fn from_password(password: String) -> Self {
        Self { password }
    }

    pub fn is_empty(&self) -> bool {
        self.password.is_empty()
    }

    pub fn into_password(self) -> String {
        self.password
    }
}

impl std::fmt::Debug for WindowsCredentialSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsCredentialSecret")
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlBackendError {
    operation_status: ControlOperationStatus,
    control_error_code: ControlErrorCode,
    message: String,
    next_recommended_action: Option<String>,
}

impl ControlBackendError {
    pub fn dashboard_status_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::DashboardStatusUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether WinFaceUnlock service and configuration are available.".to_owned(),
            ),
        }
    }

    pub fn settings_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::SettingsUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether WinFaceUnlock service configuration is available.".to_owned(),
            ),
        }
    }

    pub fn settings_persistence_failed(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::Failed,
            control_error_code: ControlErrorCode::SettingsPersistenceFailed,
            message: message.into(),
            next_recommended_action: Some(
                "Retry after confirming the service registry configuration can be updated."
                    .to_owned(),
            ),
        }
    }

    pub fn credential_enrollment_unavailable(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::ServiceUnavailable,
            control_error_code: ControlErrorCode::CredentialEnrollmentUnavailable,
            message: message.into(),
            next_recommended_action: Some(
                "Check whether the WinFaceUnlock credential store can be opened.".to_owned(),
            ),
        }
    }

    pub fn credential_enrollment_failed(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::Failed,
            control_error_code: ControlErrorCode::CredentialEnrollmentFailed,
            message: message.into(),
            next_recommended_action: Some(
                "Confirm the Windows credential and retry enrollment.".to_owned(),
            ),
        }
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::PermissionDenied,
            control_error_code: ControlErrorCode::PermissionDenied,
            message: message.into(),
            next_recommended_action: Some(
                "Run the control frontend with sufficient local privileges.".to_owned(),
            ),
        }
    }

    pub fn requires_elevation(message: impl Into<String>) -> Self {
        Self {
            operation_status: ControlOperationStatus::RequiresElevation,
            control_error_code: ControlErrorCode::ElevationRequired,
            message: message.into(),
            next_recommended_action: Some(
                "Retry from an elevated local control process or service-mediated settings path."
                    .to_owned(),
            ),
        }
    }

    fn status_reader_error(error: ControlStatusError) -> Self {
        match error {
            ControlStatusError::PermissionDenied(message) => Self::permission_denied(message),
            ControlStatusError::ElevationRequired(message) => Self::requires_elevation(message),
            ControlStatusError::ServiceStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::ServiceUnavailable,
                control_error_code: ControlErrorCode::ServiceStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check whether the Windows Service Control Manager is reachable.".to_owned(),
                ),
            },
            ControlStatusError::ProviderStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::ProviderStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock credential provider registry entries.".to_owned(),
                ),
            },
            ControlStatusError::ServiceConfigUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::ServiceConfigUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock service configuration registry entries.".to_owned(),
                ),
            },
            ControlStatusError::SettingsUnavailable(message) => Self::settings_unavailable(message),
            ControlStatusError::SettingsPersistenceFailed(message) => {
                Self::settings_persistence_failed(message)
            }
            ControlStatusError::DataDirectoryStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::DataDirectoryStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check WinFaceUnlock ProgramData directory access.".to_owned(),
                ),
            },
            ControlStatusError::PresenceRuntimeStatusUnavailable(message) => Self {
                operation_status: ControlOperationStatus::Failed,
                control_error_code: ControlErrorCode::PresenceRuntimeStatusUnavailable,
                message,
                next_recommended_action: Some(
                    "Check the WinFaceUnlock presence runtime status file.".to_owned(),
                ),
            },
        }
    }

    fn into_response(self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        ControlResponseEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: self.operation_status,
            message: self.message,
            safe_details: json!({
                "control_error_code": self.control_error_code,
            }),
            next_recommended_action: self.next_recommended_action,
        }
    }
}

impl DashboardStatusProvider for WindowsDashboardStatusProvider {
    fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError> {
        self.load_dashboard_status()
            .map_err(ControlBackendError::status_reader_error)
    }
}

impl ControlSettingsStore for WindowsControlSettingsStore {
    fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError> {
        self.load_settings()
            .map_err(ControlBackendError::status_reader_error)
    }

    fn update_settings(
        &self,
        patch: &ControlSettingsPatch,
    ) -> Result<ControlSettingsSnapshot, ControlBackendError> {
        self.update_settings(patch)
            .map_err(ControlBackendError::status_reader_error)
    }
}

pub struct ControlHandler<D, S, C> {
    dashboard_status_provider: D,
    settings_store: S,
    credential_enrollment_store: C,
}

impl<D, S, C> ControlHandler<D, S, C>
where
    D: DashboardStatusProvider,
    S: ControlSettingsStore,
    C: WindowsCredentialEnrollmentStore,
{
    pub fn new(
        dashboard_status_provider: D,
        settings_store: S,
        credential_enrollment_store: C,
    ) -> Self {
        Self {
            dashboard_status_provider,
            settings_store,
            credential_enrollment_store,
        }
    }

    pub fn handle_request(&self, request: ControlRequestEnvelope) -> ControlResponseEnvelope {
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            return ControlResponseEnvelope::unsupported_protocol(&request);
        }

        match request.operation {
            ControlOperation::GetDashboardStatus => self.handle_get_dashboard_status(&request),
            ControlOperation::GetSettings => self.handle_get_settings(&request),
            ControlOperation::UpdateSettings => self.handle_update_settings(&request),
            ControlOperation::EnrollWindowsCredential => ControlResponseEnvelope::invalid_request(
                &request,
                "enroll_windows_credential requires a credential secret side channel.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            ),
        }
    }

    pub fn handle_windows_credential_enrollment_request(
        &self,
        request: ControlRequestEnvelope,
        password_secret: WindowsCredentialSecret,
    ) -> ControlResponseEnvelope {
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            return ControlResponseEnvelope::unsupported_protocol(&request);
        }

        if request.operation != ControlOperation::EnrollWindowsCredential {
            return ControlResponseEnvelope::invalid_request(
                &request,
                "credential enrollment requests must use enroll_windows_credential.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            );
        }

        let payload = match serde_json::from_value::<WindowsCredentialEnrollmentPayload>(
            request.payload.clone(),
        ) {
            Ok(payload) if payload.has_valid_safe_fields() => payload,
            Ok(_) => {
                return ControlResponseEnvelope::invalid_request(
                    &request,
                    "credential enrollment payload contains invalid account fields.",
                    json!({
                        "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                    }),
                );
            }
            Err(error) => {
                return ControlResponseEnvelope::invalid_request(
                    &request,
                    format!("credential enrollment payload is invalid: {error}"),
                    json!({
                        "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                    }),
                );
            }
        };

        if password_secret.is_empty() {
            return ControlResponseEnvelope::invalid_request(
                &request,
                "credential enrollment requires a non-empty credential secret.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidCredentialEnrollmentRequest,
                }),
            );
        }

        match self
            .credential_enrollment_store
            .enroll_windows_credential(&payload, password_secret)
        {
            Ok(outcome) => ControlResponseEnvelope::completed(
                &request,
                "Windows credential enrolled.",
                json!(outcome),
            ),
            Err(error) => error.into_response(&request),
        }
    }

    fn handle_get_dashboard_status(
        &self,
        request: &ControlRequestEnvelope,
    ) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "get_dashboard_status does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidDashboardStatusRequest,
                }),
            );
        }

        match self.dashboard_status_provider.load_dashboard_status() {
            Ok(status) => ControlResponseEnvelope::completed(
                request,
                "Dashboard status loaded.",
                json!(status),
            ),
            Err(error) => error.into_response(request),
        }
    }

    fn handle_get_settings(&self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        if !payload_is_empty(&request.payload) {
            return ControlResponseEnvelope::invalid_request(
                request,
                "get_settings does not accept a payload.",
                json!({
                    "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                }),
            );
        }

        match self.settings_store.load_settings() {
            Ok(settings) => {
                ControlResponseEnvelope::completed(request, "Settings loaded.", json!(settings))
            }
            Err(error) => error.into_response(request),
        }
    }

    fn handle_update_settings(&self, request: &ControlRequestEnvelope) -> ControlResponseEnvelope {
        let patch = match serde_json::from_value::<ControlSettingsPatch>(request.payload.clone()) {
            Ok(patch) if patch.has_updates() => patch,
            Ok(_) => {
                return ControlResponseEnvelope::invalid_request(
                    request,
                    "update_settings requires at least one settings field.",
                    json!({
                        "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                    }),
                );
            }
            Err(error) => {
                return ControlResponseEnvelope::invalid_request(
                    request,
                    format!("update_settings payload is invalid: {error}"),
                    json!({
                        "control_error_code": ControlErrorCode::InvalidSettingsRequest,
                    }),
                );
            }
        };

        match self.settings_store.update_settings(&patch) {
            Ok(settings) => {
                ControlResponseEnvelope::completed(request, "Settings updated.", json!(settings))
            }
            Err(error) => error.into_response(request),
        }
    }
}

fn payload_is_empty(payload: &Value) -> bool {
    match payload {
        Value::Null => true,
        Value::Object(object) => object.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use control_protocol::{
        DataDirectorySummary, LogonWakeMode, PathPresence, PresenceMonitorState,
        PresenceRuntimeSummary, ProviderRegistrationState, ProviderStatusSummary,
        RegistryConfigState, ServiceConfigSummary, ServiceInstallationState, ServiceRuntimeState,
        ServiceStatusSummary, WindowsCredentialAccountType,
    };

    use super::*;

    #[derive(Clone)]
    struct FixedDashboardStatusProvider {
        result: Result<DashboardStatus, ControlBackendError>,
    }

    impl DashboardStatusProvider for FixedDashboardStatusProvider {
        fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError> {
            self.result.clone()
        }
    }

    #[derive(Clone)]
    struct RecordingSettingsStore {
        current: Rc<RefCell<Result<ControlSettingsSnapshot, ControlBackendError>>>,
        last_patch: Rc<RefCell<Option<ControlSettingsPatch>>>,
    }

    impl ControlSettingsStore for RecordingSettingsStore {
        fn load_settings(&self) -> Result<ControlSettingsSnapshot, ControlBackendError> {
            self.current.borrow().clone()
        }

        fn update_settings(
            &self,
            patch: &ControlSettingsPatch,
        ) -> Result<ControlSettingsSnapshot, ControlBackendError> {
            self.last_patch.replace(Some(patch.clone()));
            let mut current = self.current.borrow_mut();
            let settings = match current.as_mut() {
                Ok(settings) => settings,
                Err(error) => return Err(error.clone()),
            };
            if let Some(enabled) = patch.presence_lock_enabled {
                settings.presence_lock_enabled = enabled;
            }
            if let Some(logon_wake_mode) = patch.logon_wake_mode {
                settings.logon_wake_mode = Some(logon_wake_mode);
            }
            Ok(settings.clone())
        }
    }

    fn settings_store_fixture() -> RecordingSettingsStore {
        RecordingSettingsStore {
            current: Rc::new(RefCell::new(Ok(ControlSettingsSnapshot {
                presence_lock_enabled: true,
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            }))),
            last_patch: Rc::new(RefCell::new(None)),
        }
    }

    #[derive(Clone)]
    struct RecordingCredentialEnrollmentStore {
        result: Result<WindowsCredentialEnrollmentOutcome, ControlBackendError>,
        last_payload: Rc<RefCell<Option<WindowsCredentialEnrollmentPayload>>>,
        last_password: Rc<RefCell<Option<String>>>,
    }

    impl WindowsCredentialEnrollmentStore for RecordingCredentialEnrollmentStore {
        fn enroll_windows_credential(
            &self,
            payload: &WindowsCredentialEnrollmentPayload,
            password_secret: WindowsCredentialSecret,
        ) -> Result<WindowsCredentialEnrollmentOutcome, ControlBackendError> {
            self.last_payload.replace(Some(payload.clone()));
            self.last_password
                .replace(Some(password_secret.into_password()));
            self.result.clone()
        }
    }

    fn credential_enrollment_store_fixture() -> RecordingCredentialEnrollmentStore {
        RecordingCredentialEnrollmentStore {
            result: Ok(WindowsCredentialEnrollmentOutcome {
                windows_account_username: "Leo16".to_owned(),
                user_id: "dev-user".to_owned(),
                user_sid: "S-1-5-21-winfaceunlock-pending".to_owned(),
                account_type: WindowsCredentialAccountType::Local,
                credential_ref: "windows-credential-dev-user".to_owned(),
            }),
            last_payload: Rc::new(RefCell::new(None)),
            last_password: Rc::new(RefCell::new(None)),
        }
    }

    fn request(operation: ControlOperation, payload: Value) -> ControlRequestEnvelope {
        ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-c1-test".to_owned(),
            operation,
            payload,
        }
    }

    fn dashboard_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetDashboardStatus, payload)
    }

    fn settings_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::GetSettings, payload)
    }

    fn update_settings_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::UpdateSettings, payload)
    }

    fn credential_enrollment_request(payload: Value) -> ControlRequestEnvelope {
        request(ControlOperation::EnrollWindowsCredential, payload)
    }

    fn dashboard_status_fixture() -> DashboardStatus {
        DashboardStatus {
            service: ServiceStatusSummary {
                installation_state: ServiceInstallationState::Installed,
                runtime_state: ServiceRuntimeState::Running,
                process_id: Some(100),
            },
            provider: ProviderStatusSummary {
                registration_state: ProviderRegistrationState::PartiallyRegistered,
                credential_provider_registered: true,
                com_server_registered: false,
                project_config_registered: true,
            },
            service_config: ServiceConfigSummary {
                registry_config_state: RegistryConfigState::Present,
                auth_mode: Some("local_camera".to_owned()),
                face_template_path: None,
                presence_lock_enabled: Some(true),
                presence_detector_kind: Some("person".to_owned()),
                presence_tracking_mode: Some("owner_face".to_owned()),
            },
            data_directory: DataDirectorySummary {
                program_data_dir: Some("C:\\ProgramData\\WinFaceUnlock".to_owned()),
                program_data_presence: PathPresence::Present,
                presence_audit_dir: None,
                presence_audit_presence: PathPresence::Unknown,
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
    fn get_dashboard_status_returns_completed_response() -> Result<(), serde_json::Error> {
        let expected_status = dashboard_status_fixture();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(expected_status.clone()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Dashboard status loaded.");
        let decoded_status: DashboardStatus = serde_json::from_value(response.safe_details)?;
        assert_eq!(decoded_status, expected_status);
        Ok(())
    }

    #[test]
    fn get_dashboard_status_rejects_non_empty_payload() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({
            "unexpected": true,
        })));

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
    fn unsupported_protocol_returns_version_details() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );
        let mut request = dashboard_request(json!({}));
        request.protocol_version = CONTROL_PROTOCOL_VERSION + 1;

        let response = handler.handle_request(request);

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
    fn provider_error_maps_to_semantic_response() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Err(ControlBackendError::dashboard_status_unavailable(
                    "service manager is unavailable",
                )),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::ServiceUnavailable
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "dashboard_status_unavailable"
        );
        assert!(
            response
                .next_recommended_action
                .as_deref()
                .is_some_and(|action| action.contains("service"))
        );
    }

    #[test]
    fn permission_error_maps_to_permission_denied_response() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Err(ControlBackendError::permission_denied(
                    "registry access denied",
                )),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(dashboard_request(Value::Null));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::PermissionDenied
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "permission_denied"
        );
    }

    #[test]
    fn get_settings_returns_completed_response() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(settings_request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Settings loaded.");
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert!(decoded_settings.presence_lock_enabled);
        assert_eq!(
            decoded_settings.logon_wake_mode,
            Some(LogonWakeMode::InputTriggered)
        );
        Ok(())
    }

    #[test]
    fn update_settings_persists_patch_and_returns_snapshot() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let last_patch = settings_store.last_patch.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "presence_lock_enabled": false,
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Settings updated.");
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert!(!decoded_settings.presence_lock_enabled);
        assert_eq!(
            last_patch.borrow().as_ref(),
            Some(&ControlSettingsPatch {
                presence_lock_enabled: Some(false),
                logon_wake_mode: None,
            })
        );
        Ok(())
    }

    #[test]
    fn update_settings_accepts_input_triggered_logon_wake_mode() -> Result<(), serde_json::Error> {
        let settings_store = settings_store_fixture();
        let last_patch = settings_store.last_patch.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "logon_wake_mode": "input_triggered",
        })));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        let decoded_settings: ControlSettingsSnapshot =
            serde_json::from_value(response.safe_details)?;
        assert_eq!(
            decoded_settings.logon_wake_mode,
            Some(LogonWakeMode::InputTriggered)
        );
        assert_eq!(
            last_patch.borrow().as_ref(),
            Some(&ControlSettingsPatch {
                presence_lock_enabled: None,
                logon_wake_mode: Some(LogonWakeMode::InputTriggered),
            })
        );
        Ok(())
    }

    #[test]
    fn update_settings_rejects_empty_patch() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_settings_request"
        );
    }

    #[test]
    fn update_settings_elevation_error_maps_to_requires_elevation_response() {
        let settings_store = RecordingSettingsStore {
            current: Rc::new(RefCell::new(Err(ControlBackendError::requires_elevation(
                "service settings registry update requires elevation",
            )))),
            last_patch: Rc::new(RefCell::new(None)),
        };
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store,
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(update_settings_request(json!({
            "presence_lock_enabled": true,
        })));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::RequiresElevation
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "elevation_required"
        );
    }

    #[test]
    fn generic_control_request_rejects_credential_enrollment_without_secret_channel() {
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_enrollment_store_fixture(),
        );

        let response = handler.handle_request(credential_enrollment_request(json!({})));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_credential_enrollment_request"
        );
    }

    #[test]
    fn credential_enrollment_secret_channel_persists_safe_payload() -> Result<(), serde_json::Error>
    {
        let credential_store = credential_enrollment_store_fixture();
        let last_payload = credential_store.last_payload.clone();
        let last_password = credential_store.last_password.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_store,
        );

        let response = handler.handle_windows_credential_enrollment_request(
            credential_enrollment_request(json!({
                "windows_account_username": "Leo16",
                "user_id": "dev-user",
                "user_sid": "S-1-5-21-winfaceunlock-pending",
                "account_type": "local",
            })),
            WindowsCredentialSecret::from_password("secret".to_owned()),
        );

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Windows credential enrolled.");
        let decoded_outcome: WindowsCredentialEnrollmentOutcome =
            serde_json::from_value(response.safe_details.clone())?;
        assert_eq!(decoded_outcome.windows_account_username, "Leo16");
        assert_eq!(
            last_payload
                .borrow()
                .as_ref()
                .map(|payload| payload.user_id.as_str()),
            Some("dev-user")
        );
        assert_eq!(last_password.borrow().as_deref(), Some("secret"));
        assert!(!serde_json::to_string(&response)?.contains("secret"));
        Ok(())
    }

    #[test]
    fn credential_enrollment_rejects_empty_secret() {
        let credential_store = credential_enrollment_store_fixture();
        let last_payload = credential_store.last_payload.clone();
        let handler = ControlHandler::new(
            FixedDashboardStatusProvider {
                result: Ok(dashboard_status_fixture()),
            },
            settings_store_fixture(),
            credential_store,
        );

        let response = handler.handle_windows_credential_enrollment_request(
            credential_enrollment_request(json!({})),
            WindowsCredentialSecret::from_password(String::new()),
        );

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::InvalidRequest
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "invalid_credential_enrollment_request"
        );
        assert_eq!(last_payload.borrow().as_ref(), None);
    }
}
