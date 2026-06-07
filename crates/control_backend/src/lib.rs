use control_protocol::{
    CONTROL_PROTOCOL_VERSION, ControlErrorCode, ControlOperation, ControlOperationStatus,
    ControlRequestEnvelope, ControlResponseEnvelope, DashboardStatus,
};
use serde_json::{Value, json};

pub trait DashboardStatusProvider {
    fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlBackendError>;
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

pub struct ControlHandler<P> {
    dashboard_status_provider: P,
}

impl<P> ControlHandler<P>
where
    P: DashboardStatusProvider,
{
    pub fn new(dashboard_status_provider: P) -> Self {
        Self {
            dashboard_status_provider,
        }
    }

    pub fn handle_request(&self, request: ControlRequestEnvelope) -> ControlResponseEnvelope {
        if request.protocol_version != CONTROL_PROTOCOL_VERSION {
            return ControlResponseEnvelope::unsupported_protocol(&request);
        }

        match request.operation {
            ControlOperation::GetDashboardStatus => self.handle_get_dashboard_status(&request),
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
    use control_protocol::{
        DataDirectorySummary, PathPresence, PresenceMonitorState, PresenceRuntimeSummary,
        ProviderRegistrationState, ProviderStatusSummary, RegistryConfigState,
        ServiceConfigSummary, ServiceInstallationState, ServiceRuntimeState, ServiceStatusSummary,
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

    fn request(payload: Value) -> ControlRequestEnvelope {
        ControlRequestEnvelope {
            protocol_version: CONTROL_PROTOCOL_VERSION,
            correlation_id: "control-c1-test".to_owned(),
            operation: ControlOperation::GetDashboardStatus,
            payload,
        }
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
        let handler = ControlHandler::new(FixedDashboardStatusProvider {
            result: Ok(expected_status.clone()),
        });

        let response = handler.handle_request(request(json!({})));

        assert_eq!(response.operation_status, ControlOperationStatus::Completed);
        assert_eq!(response.message, "Dashboard status loaded.");
        let decoded_status: DashboardStatus = serde_json::from_value(response.safe_details)?;
        assert_eq!(decoded_status, expected_status);
        Ok(())
    }

    #[test]
    fn get_dashboard_status_rejects_non_empty_payload() {
        let handler = ControlHandler::new(FixedDashboardStatusProvider {
            result: Ok(dashboard_status_fixture()),
        });

        let response = handler.handle_request(request(json!({
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
        let handler = ControlHandler::new(FixedDashboardStatusProvider {
            result: Ok(dashboard_status_fixture()),
        });
        let mut request = request(json!({}));
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
        let handler = ControlHandler::new(FixedDashboardStatusProvider {
            result: Err(ControlBackendError::dashboard_status_unavailable(
                "service manager is unavailable",
            )),
        });

        let response = handler.handle_request(request(json!({})));

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
        let handler = ControlHandler::new(FixedDashboardStatusProvider {
            result: Err(ControlBackendError::permission_denied(
                "registry access denied",
            )),
        });

        let response = handler.handle_request(request(Value::Null));

        assert_eq!(
            response.operation_status,
            ControlOperationStatus::PermissionDenied
        );
        assert_eq!(
            response.safe_details["control_error_code"],
            "permission_denied"
        );
    }
}
