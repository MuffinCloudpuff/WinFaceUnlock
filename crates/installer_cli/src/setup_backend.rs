use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

use common_protocol::AccountType;
#[cfg(all(windows, not(test)))]
use common_protocol::{CredentialRef, UserId};
use control_protocol::{
    DashboardStatus, PathPresence, ProviderRegistrationState, RegistryConfigState,
    ServiceInstallationState, ServiceRuntimeState,
};
use control_status::{ControlStatusError, WindowsDashboardStatusProvider};
use serde_json::{Value, json};
use setup_api::{
    ConfigurePresenceLockPayload, EnrollCredentialPayload, EnrollFaceTemplatePayload,
    InspectPayloadPayload, InstallSystemComponentsPayload, OperationStatus, PreflightPayload,
    RunAuthSelfTestPayload, SETUP_PROTOCOL_VERSION, SetupErrorCode, SetupOperation,
    SetupRequestEnvelope, SetupResponseEnvelope, SetupStepStatus, StagePayloadPayload,
    UninstallPayload,
};
#[cfg(all(windows, not(test)))]
use win_service::credential_store_config::{
    ServiceCredentialStorePaths, WindowsCredentialEnrollment, enroll_windows_credential,
};

use crate::{
    installation::{FullInstallPlan, FullUninstallPlan, InstallerOrchestrator},
    resource_directory::ResourceDirectoryPlan,
    service_manager::InstallerError,
    service_registry::ServiceAuthRegistry,
};

pub fn run_from_stdio() -> Result<(), InstallerError> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let output = run_from_json_text(&input)?;
    std::io::stdout().write_all(output.as_bytes())?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}

fn run_from_json_text(input: &str) -> Result<String, InstallerError> {
    let request = serde_json::from_str::<SetupRequestEnvelope>(input).map_err(|error| {
        InstallerError::InvalidArguments(format!("invalid setup JSON: {error}"))
    })?;
    let response = handle_request(&request);
    serde_json::to_string(&response)
        .map_err(|error| InstallerError::InvalidArguments(format!("serialize setup JSON: {error}")))
}

fn handle_request(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    if request.protocol_version != SETUP_PROTOCOL_VERSION {
        return SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::UnsupportedProtocol,
            message: format!(
                "Unsupported setup protocol version {}.",
                request.protocol_version
            ),
            safe_details: json!({
                "supported_protocol_version": SETUP_PROTOCOL_VERSION,
            }),
            next_recommended_action: Some("Update the setup frontend or backend.".to_owned()),
        };
    }

    match request.operation {
        SetupOperation::GetStatus => get_status(request),
        SetupOperation::RunPreflight => run_preflight(request),
        SetupOperation::InspectPayload => inspect_payload(request),
        SetupOperation::StagePayload => stage_payload(request),
        SetupOperation::EnrollCredential => enroll_credential(request),
        SetupOperation::EnrollFaceTemplate => enroll_face_template(request),
        SetupOperation::RunAuthSelfTest => run_auth_self_test(request),
        SetupOperation::InstallSystemComponents => install_system_components(request),
        SetupOperation::ConfigurePresenceLock => configure_presence_lock(request),
        SetupOperation::Repair => repair(request),
        SetupOperation::EmergencyDisable => emergency_disable(request),
        SetupOperation::Uninstall => uninstall(request),
    }
}

fn inspect_payload(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<InspectPayloadPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid inspect payload request: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "inspect_payload",
                }),
            );
        }
    };

    match crate::setup_payload::inspect_payload(&payload) {
        Ok(outcome) => {
            let inspected_files = outcome
                .inspected_files
                .iter()
                .map(|file| {
                    json!({
                        "file_id": file.file_id,
                        "source_relative_path": file.source_relative_path,
                        "target_relative_path": file.target_relative_path,
                        "source_path": file.source_path,
                        "required": file.required,
                        "payload_file_presence_status": file.payload_file_presence_status.as_str(),
                    })
                })
                .collect::<Vec<_>>();
            let missing_required_payload_files = outcome
                .missing_required_payload_files
                .iter()
                .map(|file| {
                    json!({
                        "file_id": file.file_id,
                        "path": file.path,
                    })
                })
                .collect::<Vec<_>>();
            let stage_payload_files = outcome
                .stage_payload_files
                .iter()
                .map(|file| {
                    json!({
                        "file_id": file.file_id,
                        "source_path": file.source_path,
                        "target_relative_path": file.target_relative_path,
                    })
                })
                .collect::<Vec<_>>();
            let safe_details = json!({
                "payload_root_dir": payload.payload_root_dir,
                "manifest_path": outcome.manifest_path,
                "inspected_files": inspected_files,
                "missing_required_payload_files": missing_required_payload_files,
                "stage_payload_files": stage_payload_files,
            });

            if outcome.missing_required_payload_files.is_empty() {
                SetupResponseEnvelope::succeeded(
                    request,
                    "Payload manifest inspected.",
                    safe_details,
                )
            } else {
                SetupResponseEnvelope {
                    protocol_version: SETUP_PROTOCOL_VERSION,
                    correlation_id: request.correlation_id.clone(),
                    operation: request.operation,
                    operation_status: OperationStatus::Failed,
                    message: "Payload manifest is missing required files.".to_owned(),
                    safe_details: json!({
                        "setup_error_code": SetupErrorCode::MissingPayloadFile,
                        "payload_root_dir": safe_details["payload_root_dir"].clone(),
                        "manifest_path": safe_details["manifest_path"].clone(),
                        "inspected_files": safe_details["inspected_files"].clone(),
                        "missing_required_payload_files": safe_details["missing_required_payload_files"].clone(),
                        "stage_payload_files": safe_details["stage_payload_files"].clone(),
                    }),
                    next_recommended_action: Some(
                        "Rebuild or re-extract the setup payload and retry.".to_owned(),
                    ),
                }
            }
        }
        Err(error) => {
            let setup_error_code = if error.is_missing_source_file() {
                SetupErrorCode::MissingPayloadFile
            } else if error.is_invalid_request() {
                SetupErrorCode::InvalidRequest
            } else {
                SetupErrorCode::StagePayloadFailed
            };
            SetupResponseEnvelope::failed(
                request,
                format!("Payload manifest inspection failed: {error}"),
                setup_error_code,
            )
        }
    }
}

fn get_status(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    match load_status_details() {
        Ok(details) => SetupResponseEnvelope::succeeded(request, "Status loaded.", details),
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("Failed to load setup status: {error}"),
            SetupErrorCode::InvalidRequest,
        ),
    }
}

fn load_status_details() -> Result<Value, ControlStatusError> {
    let status =
        WindowsDashboardStatusProvider::from_environment_or_default().load_dashboard_status()?;
    Ok(legacy_setup_status_details(&status))
}

fn legacy_setup_status_details(status: &DashboardStatus) -> Value {
    json!({
        "service": {
            "installed": status.service.installation_state == ServiceInstallationState::Installed,
            "running": status.service.runtime_state == ServiceRuntimeState::Running,
            "state": service_state_text(&status.service.runtime_state),
            "process_id": status.service.process_id,
        },
        "provider": {
            "credential_provider_registered": status.provider.credential_provider_registered,
            "com_server_registered": status.provider.com_server_registered,
            "project_config_registered": status.provider.project_config_registered,
            "registered": status.provider.registration_state == ProviderRegistrationState::Registered,
        },
        "service_config": {
            "registry_config_exists": status.service_config.registry_config_state == RegistryConfigState::Present,
            "auth_mode": status.service_config.auth_mode,
            "face_template_path": status.service_config.face_template_path,
            "presence_lock_enabled": legacy_optional_bool(status.service_config.presence_lock_enabled),
            "presence_detector_kind": status.service_config.presence_detector_kind,
            "presence_tracking_mode": status.service_config.presence_tracking_mode,
            "presence_person_detector_model": null,
        },
        "data": {
            "program_data_dir": status.data_directory.program_data_dir,
            "program_data_exists": status.data_directory.program_data_presence == PathPresence::Present,
            "presence_audit_dir": status.data_directory.presence_audit_dir,
            "presence_audit_dir_exists": status.data_directory.presence_audit_presence == PathPresence::Present,
        }
    })
}

fn service_state_text(state: &ServiceRuntimeState) -> String {
    match state {
        ServiceRuntimeState::Running => "Running".to_owned(),
        ServiceRuntimeState::Stopped => "Stopped".to_owned(),
        ServiceRuntimeState::Paused => "Paused".to_owned(),
        ServiceRuntimeState::StartPending => "StartPending".to_owned(),
        ServiceRuntimeState::StopPending => "StopPending".to_owned(),
        ServiceRuntimeState::Missing => "missing".to_owned(),
        ServiceRuntimeState::Unknown(value) => value.clone(),
    }
}

fn legacy_optional_bool(value: Option<bool>) -> Option<&'static str> {
    value.map(|value| if value { "true" } else { "false" })
}

fn run_preflight(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<PreflightPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid preflight payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "run_preflight",
                }),
            );
        }
    };

    let outcome = crate::setup_preflight::run_preflight(&payload);
    let failed_check_ids = outcome
        .checks
        .iter()
        .filter(|check| check.status == SetupStepStatus::Failed)
        .map(|check| check.check_id.clone())
        .collect::<Vec<_>>();
    let missing_payload_files = outcome
        .missing_payload_files
        .iter()
        .map(|file| {
            json!({
                "file_id": file.file_id,
                "path": file.path,
            })
        })
        .collect::<Vec<_>>();
    let safe_details = json!({
        "install_dir": payload.install_dir,
        "checks": outcome.checks,
        "failed_check_ids": failed_check_ids,
        "missing_payload_files": missing_payload_files,
    });

    if outcome.all_required_checks_passed() {
        return SetupResponseEnvelope::succeeded(request, "Preflight checks passed.", safe_details);
    }

    if outcome.requires_elevation() {
        return SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::RequiresElevation,
            message: "Setup backend requires elevation for this operation.".to_owned(),
            safe_details,
            next_recommended_action: Some("Restart setup with UAC elevation.".to_owned()),
        };
    }

    SetupResponseEnvelope {
        protocol_version: SETUP_PROTOCOL_VERSION,
        correlation_id: request.correlation_id.clone(),
        operation: request.operation,
        operation_status: OperationStatus::Failed,
        message: "Preflight checks failed.".to_owned(),
        safe_details,
        next_recommended_action: Some("Resolve failed preflight checks and retry.".to_owned()),
    }
}

fn stage_payload(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<StagePayloadPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid stage payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "stage_payload",
                }),
            );
        }
    };

    if payload.payload_files.is_empty() {
        return SetupResponseEnvelope::invalid_request(
            request,
            "Stage payload requires at least one payload file.",
            json!({
                "setup_error_code": SetupErrorCode::InvalidRequest,
                "payload_type": "stage_payload",
            }),
        );
    }

    let required_payload_files =
        match crate::setup_payload::required_payload_files_for_preflight(&payload) {
            Ok(required_payload_files) => required_payload_files,
            Err(error) => {
                if error.is_invalid_request() {
                    return SetupResponseEnvelope::invalid_request(
                        request,
                        format!("Invalid stage payload: {error}"),
                        json!({
                            "setup_error_code": SetupErrorCode::InvalidRequest,
                            "payload_type": "stage_payload",
                        }),
                    );
                }
                return SetupResponseEnvelope::failed(
                    request,
                    format!("Payload staging request failed: {error}"),
                    SetupErrorCode::StagePayloadFailed,
                );
            }
        };

    let preflight_payload = PreflightPayload {
        install_dir: payload.install_dir.clone(),
        require_elevation: false,
        required_payload_files,
    };
    let preflight_outcome = crate::setup_preflight::run_preflight(&preflight_payload);
    if !preflight_outcome.all_required_checks_passed() {
        return setup_preflight_failed_response(request, &payload, preflight_outcome);
    }

    match crate::setup_payload::stage_payload(&payload) {
        Ok(outcome) => {
            let staged_files = outcome
                .staged_files
                .iter()
                .map(|file| {
                    json!({
                        "file_id": file.file_id,
                        "source_path": file.source_path,
                        "target_path": file.target_path,
                        "stage_file_status": file.stage_file_status.as_str(),
                    })
                })
                .collect::<Vec<_>>();
            SetupResponseEnvelope::succeeded(
                request,
                "Payload staged.",
                json!({
                    "install_dir": payload.install_dir,
                    "staged_files": staged_files,
                }),
            )
        }
        Err(error) if error.is_provider_dll_in_place_overwrite_blocked() => SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::BlockedByRunningProcess,
            message: format!("Payload staging blocked: {error}"),
            safe_details: json!({
                "setup_error_code": SetupErrorCode::StagePayloadFailed,
                "blocked_reason": "provider_dll_in_place_overwrite_not_allowed",
            }),
            next_recommended_action: Some(
                "Stage Provider DLL with a versioned target name or uninstall first.".to_owned(),
            ),
        },
        Err(error) => {
            let setup_error_code = if error.is_missing_source_file() {
                SetupErrorCode::MissingPayloadFile
            } else if error.is_invalid_install_dir() {
                SetupErrorCode::InvalidInstallDir
            } else {
                SetupErrorCode::StagePayloadFailed
            };
            SetupResponseEnvelope::failed(
                request,
                format!("Payload staging failed: {error}"),
                setup_error_code,
            )
        }
    }
}

fn setup_preflight_failed_response(
    request: &SetupRequestEnvelope,
    payload: &StagePayloadPayload,
    outcome: crate::setup_preflight::PreflightOutcome,
) -> SetupResponseEnvelope {
    let failed_check_ids = outcome
        .checks
        .iter()
        .filter(|check| check.status == SetupStepStatus::Failed)
        .map(|check| check.check_id.clone())
        .collect::<Vec<_>>();
    let missing_payload_files = outcome
        .missing_payload_files
        .iter()
        .map(|file| {
            json!({
                "file_id": file.file_id,
                "path": file.path,
            })
        })
        .collect::<Vec<_>>();
    SetupResponseEnvelope {
        protocol_version: SETUP_PROTOCOL_VERSION,
        correlation_id: request.correlation_id.clone(),
        operation: request.operation,
        operation_status: OperationStatus::Failed,
        message: "Payload staging preflight checks failed.".to_owned(),
        safe_details: json!({
            "setup_error_code": SetupErrorCode::StagePayloadFailed,
            "install_dir": payload.install_dir,
            "checks": outcome.checks,
            "failed_check_ids": failed_check_ids,
            "missing_payload_files": missing_payload_files,
        }),
        next_recommended_action: Some(
            "Resolve failed preflight checks and retry staging.".to_owned(),
        ),
    }
}

fn install_system_components(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload =
        match serde_json::from_value::<InstallSystemComponentsPayload>(request.payload.clone()) {
            Ok(payload) => payload,
            Err(error) => {
                return SetupResponseEnvelope::invalid_request(
                    request,
                    format!("Invalid install system components payload: {error}"),
                    json!({
                        "setup_error_code": SetupErrorCode::InvalidRequest,
                        "payload_type": "install_system_components",
                    }),
                );
            }
        };

    let plan = match build_install_plan_response(request, &payload, "Install system components") {
        Ok(plan) => plan,
        Err(response) => return response,
    };
    let safe_details = install_plan_safe_details(&payload, &plan);

    match InstallerOrchestrator::install(&plan) {
        Ok(()) => {
            SetupResponseEnvelope::succeeded(request, "System components installed.", safe_details)
        }
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("System component install failed: {error}"),
            SetupErrorCode::ServiceInstallFailed,
        ),
    }
}

fn repair(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload =
        match serde_json::from_value::<InstallSystemComponentsPayload>(request.payload.clone()) {
            Ok(payload) => payload,
            Err(error) => {
                return SetupResponseEnvelope::invalid_request(
                    request,
                    format!("Invalid repair payload: {error}"),
                    json!({
                        "setup_error_code": SetupErrorCode::InvalidRequest,
                        "payload_type": "repair",
                    }),
                );
            }
        };

    let plan = match build_install_plan_response(request, &payload, "Repair") {
        Ok(plan) => plan,
        Err(response) => return response,
    };
    let safe_details = install_plan_safe_details(&payload, &plan);

    match InstallerOrchestrator::repair(&plan) {
        Ok(()) => {
            SetupResponseEnvelope::succeeded(request, "System components repaired.", safe_details)
        }
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("System component repair failed: {error}"),
            SetupErrorCode::RepairFailed,
        ),
    }
}

fn build_install_plan_response(
    request: &SetupRequestEnvelope,
    payload: &InstallSystemComponentsPayload,
    operation_name: &str,
) -> Result<FullInstallPlan, SetupResponseEnvelope> {
    crate::setup_install::build_install_plan(payload).map_err(|error| {
        let setup_error_code = if error.is_invalid_install_dir() {
            SetupErrorCode::InvalidInstallDir
        } else if error.is_missing_required_file() {
            SetupErrorCode::MissingPayloadFile
        } else {
            SetupErrorCode::InvalidRequest
        };
        SetupResponseEnvelope::failed(
            request,
            format!("{operation_name} plan failed: {error}"),
            setup_error_code,
        )
    })
}

fn install_plan_safe_details(
    payload: &InstallSystemComponentsPayload,
    plan: &FullInstallPlan,
) -> Value {
    json!({
        "install_dir": payload.install_dir,
        "service_binary_path": plan.service_plan.service_binary_path,
        "provider_binary_path": plan.provider_plan.provider_binary_path,
        "face_template_path": plan.auth_config.as_ref().map(|config| &config.face_template_path),
        "yunet_model_path": plan.auth_config.as_ref().map(|config| &config.yunet_model_path),
        "sface_model_path": plan.auth_config.as_ref().map(|config| &config.sface_model_path),
        "minifasnet_model_path": plan.auth_config.as_ref().map(|config| &config.minifasnet_model_path),
        "start_service": plan.start_service,
        "provider_mode": {
            "wake_auth_source": plan.provider_plan.wake_auth_source,
            "tile_visibility": plan.provider_plan.tile_visibility,
            "auto_wake_on_advise": plan.provider_plan.auto_wake_on_advise,
        },
    })
}

fn enroll_credential(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<EnrollCredentialPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid credential enrollment payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "enroll_credential",
                }),
            );
        }
    };

    let account_type = match parse_account_type(&payload.account_type) {
        Ok(account_type) => account_type,
        Err(message) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                message,
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "enroll_credential",
                    "field": "account_type",
                }),
            );
        }
    };
    let credential_ref = payload
        .credential_ref
        .clone()
        .unwrap_or_else(|| format!("windows-credential-{}", payload.user_id));
    let safe_details = json!({
        "username": payload.username,
        "user_id": payload.user_id,
        "user_sid": payload.user_sid,
        "account_type": payload.account_type,
        "credential_ref": credential_ref,
        "store_dir": payload.store_dir,
        "password_transport": credential_password_transport_label(&payload),
    });

    match enroll_credential_from_payload(&payload, account_type, &credential_ref) {
        Ok(outcome) => SetupResponseEnvelope::succeeded(
            request,
            "Windows credential enrolled.",
            json!({
                "username": safe_details["username"].clone(),
                "user_id": safe_details["user_id"].clone(),
                "user_sid": safe_details["user_sid"].clone(),
                "account_type": safe_details["account_type"].clone(),
                "credential_ref": safe_details["credential_ref"].clone(),
                "store_dir": safe_details["store_dir"].clone(),
                "password_transport": safe_details["password_transport"].clone(),
                "credential_store_database": outcome.credential_store_database,
                "credential_store_master_key": outcome.credential_store_master_key,
                "prompt_username": outcome.prompt_username,
            }),
        ),
        Err(CredentialEnrollmentError::Cancelled) => SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Cancelled,
            message: "Credential enrollment was cancelled.".to_owned(),
            safe_details,
            next_recommended_action: Some("Run credential enrollment again.".to_owned()),
        },
        Err(error) => SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Failed,
            message: format!("Credential enrollment failed: {error}"),
            safe_details: json!({
                "setup_error_code": SetupErrorCode::CredentialEnrollmentFailed,
                "details": safe_details,
            }),
            next_recommended_action: Some(
                "Confirm the Windows credential and retry enrollment.".to_owned(),
            ),
        },
    }
}

fn parse_account_type(account_type: &str) -> Result<AccountType, String> {
    match account_type {
        "local" => Ok(AccountType::Local),
        "microsoft" | "microsoft-account" => Ok(AccountType::MicrosoftAccount),
        "domain" => Ok(AccountType::Domain),
        value => Err(format!(
            "Invalid credential enrollment account type: {value}"
        )),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CredentialEnrollmentOutcome {
    credential_store_database: PathBuf,
    credential_store_master_key: PathBuf,
    prompt_username: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum CredentialEnrollmentError {
    Cancelled,
    NativePromptUnavailable,
    NativePromptFailed(u32),
    InvalidSecretTransport(String),
    SecretTransportUnavailable,
    SecretTransportFailed(String),
    SecretTransportNonceMismatch,
    EmptyPassword,
    StoreWriteFailed(String),
}

impl std::fmt::Display for CredentialEnrollmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(formatter, "native credential prompt cancelled"),
            Self::NativePromptUnavailable => {
                write!(formatter, "native credential prompt unavailable")
            }
            Self::NativePromptFailed(code) => {
                write!(
                    formatter,
                    "native credential prompt failed with error {code}"
                )
            }
            Self::InvalidSecretTransport(message) => {
                write!(formatter, "invalid credential secret transport: {message}")
            }
            Self::SecretTransportUnavailable => {
                write!(formatter, "credential secret transport unavailable")
            }
            Self::SecretTransportFailed(error) => {
                write!(formatter, "credential secret transport failed: {error}")
            }
            Self::SecretTransportNonceMismatch => {
                write!(formatter, "credential secret transport nonce mismatch")
            }
            Self::EmptyPassword => write!(
                formatter,
                "native credential prompt returned empty password"
            ),
            Self::StoreWriteFailed(error) => {
                write!(formatter, "credential store write failed: {error}")
            }
        }
    }
}

fn credential_password_transport_label(payload: &EnrollCredentialPayload) -> &str {
    payload
        .password_secret_transport
        .as_ref()
        .map(|transport| transport.transport_kind.as_str())
        .unwrap_or("native_windows_credential_prompt")
}

#[cfg(all(windows, not(test)))]
fn enroll_credential_from_payload(
    payload: &EnrollCredentialPayload,
    account_type: AccountType,
    credential_ref: &str,
) -> Result<CredentialEnrollmentOutcome, CredentialEnrollmentError> {
    match &payload.password_secret_transport {
        Some(transport) => enroll_credential_with_secret_transport(
            payload,
            account_type,
            credential_ref,
            transport,
        ),
        None => enroll_credential_with_native_prompt(payload, account_type, credential_ref),
    }
}

#[cfg(any(not(windows), test))]
fn enroll_credential_from_payload(
    payload: &EnrollCredentialPayload,
    account_type: AccountType,
    credential_ref: &str,
) -> Result<CredentialEnrollmentOutcome, CredentialEnrollmentError> {
    if payload.password_secret_transport.is_some() {
        return Err(CredentialEnrollmentError::SecretTransportUnavailable);
    }
    enroll_credential_with_native_prompt(payload, account_type, credential_ref)
}

#[cfg(all(windows, not(test)))]
fn enroll_credential_with_secret_transport(
    payload: &EnrollCredentialPayload,
    account_type: AccountType,
    credential_ref: &str,
    transport: &setup_api::CredentialSecretTransportPayload,
) -> Result<CredentialEnrollmentOutcome, CredentialEnrollmentError> {
    let password = credential_secret_pipe::read_password(transport)?;
    if password.is_empty() {
        return Err(CredentialEnrollmentError::EmptyPassword);
    }

    let store_paths = payload
        .store_dir
        .clone()
        .map(ServiceCredentialStorePaths::from_store_dir)
        .unwrap_or_else(ServiceCredentialStorePaths::from_environment_or_default);
    enroll_windows_credential(
        &store_paths,
        WindowsCredentialEnrollment {
            user_id: UserId(payload.user_id.clone()),
            user_sid: payload.user_sid.clone(),
            username: payload.username.clone(),
            account_type,
            credential_ref: CredentialRef(credential_ref.to_owned()),
            password,
        },
    )
    .map_err(|error| CredentialEnrollmentError::StoreWriteFailed(format!("{error:?}")))?;

    Ok(CredentialEnrollmentOutcome {
        credential_store_database: store_paths.database_path,
        credential_store_master_key: store_paths.master_key_path,
        prompt_username: payload.username.clone(),
    })
}

#[cfg(all(windows, not(test)))]
fn enroll_credential_with_native_prompt(
    payload: &EnrollCredentialPayload,
    account_type: AccountType,
    credential_ref: &str,
) -> Result<CredentialEnrollmentOutcome, CredentialEnrollmentError> {
    let prompt = native_credential_prompt::prompt_for_windows_password(&payload.username)?;
    if prompt.password.is_empty() {
        return Err(CredentialEnrollmentError::EmptyPassword);
    }

    let store_paths = payload
        .store_dir
        .clone()
        .map(ServiceCredentialStorePaths::from_store_dir)
        .unwrap_or_else(ServiceCredentialStorePaths::from_environment_or_default);
    enroll_windows_credential(
        &store_paths,
        WindowsCredentialEnrollment {
            user_id: UserId(payload.user_id.clone()),
            user_sid: payload.user_sid.clone(),
            username: payload.username.clone(),
            account_type,
            credential_ref: CredentialRef(credential_ref.to_owned()),
            password: prompt.password,
        },
    )
    .map_err(|error| CredentialEnrollmentError::StoreWriteFailed(format!("{error:?}")))?;

    Ok(CredentialEnrollmentOutcome {
        credential_store_database: store_paths.database_path,
        credential_store_master_key: store_paths.master_key_path,
        prompt_username: prompt.username,
    })
}

#[cfg(any(not(windows), test))]
fn enroll_credential_with_native_prompt(
    _payload: &EnrollCredentialPayload,
    _account_type: AccountType,
    _credential_ref: &str,
) -> Result<CredentialEnrollmentOutcome, CredentialEnrollmentError> {
    Err(CredentialEnrollmentError::NativePromptUnavailable)
}

#[cfg(all(windows, not(test)))]
mod credential_secret_pipe {
    use std::{
        fs::File,
        io::Read,
        thread,
        time::{Duration, Instant},
    };

    use setup_api::CredentialSecretTransportPayload;

    use super::CredentialEnrollmentError;

    const TRANSPORT_KIND: &str = "windows_named_pipe_utf8_v1";
    const PIPE_NAME_PREFIX: &str = "winfaceunlock-credential-";
    const SECRET_MAGIC: &[u8] = b"WFU_SECRET_PIPE_V1";

    pub fn read_password(
        transport: &CredentialSecretTransportPayload,
    ) -> Result<String, CredentialEnrollmentError> {
        validate_transport(transport)?;

        let mut pipe = open_pipe_with_timeout(&transport.pipe_name, transport.timeout_ms)?;
        let mut secret_bytes = Vec::new();
        pipe.read_to_end(&mut secret_bytes)
            .map_err(|error| CredentialEnrollmentError::SecretTransportFailed(error.to_string()))?;

        let password = parse_secret_bytes(&secret_bytes, &transport.secret_nonce);
        zero_byte_buffer(&mut secret_bytes);
        password
    }

    fn validate_transport(
        transport: &CredentialSecretTransportPayload,
    ) -> Result<(), CredentialEnrollmentError> {
        if transport.transport_kind != TRANSPORT_KIND {
            return Err(CredentialEnrollmentError::InvalidSecretTransport(format!(
                "unsupported transport kind {}",
                transport.transport_kind
            )));
        }

        if transport.pipe_name.len() > 160
            || !transport.pipe_name.starts_with(PIPE_NAME_PREFIX)
            || !transport
                .pipe_name
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || value == '-')
        {
            return Err(CredentialEnrollmentError::InvalidSecretTransport(
                "pipe name must use the WinFaceUnlock credential prefix and safe ASCII characters"
                    .to_owned(),
            ));
        }

        if transport.secret_nonce.len() < 16
            || !transport
                .secret_nonce
                .chars()
                .all(|value| value.is_ascii_hexdigit())
        {
            return Err(CredentialEnrollmentError::InvalidSecretTransport(
                "secret nonce must be a hex token".to_owned(),
            ));
        }

        Ok(())
    }

    fn open_pipe_with_timeout(
        pipe_name: &str,
        timeout_ms: u64,
    ) -> Result<File, CredentialEnrollmentError> {
        let pipe_path = format!(r"\\.\pipe\{pipe_name}");
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let deadline = Instant::now() + timeout;

        loop {
            match File::open(&pipe_path) {
                Ok(pipe) => return Ok(pipe),
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(CredentialEnrollmentError::SecretTransportFailed(
                        error.to_string(),
                    ));
                }
            }
        }
    }

    fn parse_secret_bytes(
        secret_bytes: &[u8],
        expected_nonce: &str,
    ) -> Result<String, CredentialEnrollmentError> {
        let first_newline = secret_bytes
            .iter()
            .position(|value| *value == b'\n')
            .ok_or_else(|| {
                CredentialEnrollmentError::SecretTransportFailed(
                    "missing protocol header".to_owned(),
                )
            })?;
        let second_newline = secret_bytes[first_newline + 1..]
            .iter()
            .position(|value| *value == b'\n')
            .map(|offset| first_newline + 1 + offset)
            .ok_or_else(|| {
                CredentialEnrollmentError::SecretTransportFailed("missing nonce header".to_owned())
            })?;

        if &secret_bytes[..first_newline] != SECRET_MAGIC {
            return Err(CredentialEnrollmentError::SecretTransportFailed(
                "invalid protocol header".to_owned(),
            ));
        }

        let nonce = std::str::from_utf8(&secret_bytes[first_newline + 1..second_newline])
            .map_err(|error| CredentialEnrollmentError::SecretTransportFailed(error.to_string()))?;
        if nonce != expected_nonce {
            return Err(CredentialEnrollmentError::SecretTransportNonceMismatch);
        }

        String::from_utf8(secret_bytes[second_newline + 1..].to_vec())
            .map_err(|error| CredentialEnrollmentError::SecretTransportFailed(error.to_string()))
    }

    fn zero_byte_buffer(buffer: &mut [u8]) {
        for value in buffer {
            *value = 0;
        }
    }
}

#[cfg(all(windows, not(test)))]
mod native_credential_prompt {
    #![allow(unsafe_code)]

    use windows_sys::Win32::{
        Foundation::ERROR_CANCELLED,
        Security::Credentials::{
            CREDUI_FLAGS_ALWAYS_SHOW_UI, CREDUI_FLAGS_DO_NOT_PERSIST,
            CREDUI_FLAGS_GENERIC_CREDENTIALS, CREDUI_FLAGS_KEEP_USERNAME, CREDUI_INFOW,
            CREDUI_MAX_USERNAME_LENGTH, CredUIPromptForCredentialsW,
        },
    };

    use super::CredentialEnrollmentError;

    const PASSWORD_BUFFER_CHARS: usize = 256;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct NativeCredentialPromptResult {
        pub username: String,
        pub password: String,
    }

    pub fn prompt_for_windows_password(
        default_username: &str,
    ) -> Result<NativeCredentialPromptResult, CredentialEnrollmentError> {
        let target_name = wide_null("WinFaceUnlock Credential Store");
        let caption = wide_null("WinFaceUnlock Setup");
        let message = wide_null(
            "Enter the Windows password that WinFaceUnlock should use after face authentication.",
        );
        let mut username_buffer =
            fixed_wide_buffer(default_username, CREDUI_MAX_USERNAME_LENGTH as usize + 1);
        let mut password_buffer = vec![0_u16; PASSWORD_BUFFER_CHARS + 1];
        let mut save = 0;
        let ui_info = CREDUI_INFOW {
            cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
            hwndParent: std::ptr::null_mut(),
            pszMessageText: message.as_ptr(),
            pszCaptionText: caption.as_ptr(),
            hbmBanner: std::ptr::null_mut(),
        };

        let status = unsafe {
            CredUIPromptForCredentialsW(
                &ui_info,
                target_name.as_ptr(),
                std::ptr::null(),
                0,
                username_buffer.as_mut_ptr(),
                username_buffer.len() as u32,
                password_buffer.as_mut_ptr(),
                password_buffer.len() as u32,
                &mut save,
                CREDUI_FLAGS_ALWAYS_SHOW_UI
                    | CREDUI_FLAGS_DO_NOT_PERSIST
                    | CREDUI_FLAGS_GENERIC_CREDENTIALS
                    | CREDUI_FLAGS_KEEP_USERNAME,
            )
        };
        if status == ERROR_CANCELLED {
            zero_wide_buffer(&mut password_buffer);
            return Err(CredentialEnrollmentError::Cancelled);
        }
        if status != 0 {
            zero_wide_buffer(&mut password_buffer);
            return Err(CredentialEnrollmentError::NativePromptFailed(status));
        }

        let username = string_from_wide_buffer(&username_buffer);
        let password = string_from_wide_buffer(&password_buffer);
        zero_wide_buffer(&mut password_buffer);

        Ok(NativeCredentialPromptResult { username, password })
    }

    fn fixed_wide_buffer(value: &str, len: usize) -> Vec<u16> {
        let mut buffer = vec![0_u16; len];
        for (index, code_unit) in value.encode_utf16().take(len.saturating_sub(1)).enumerate() {
            buffer[index] = code_unit;
        }
        buffer
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn string_from_wide_buffer(buffer: &[u16]) -> String {
        let len = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    }

    fn zero_wide_buffer(buffer: &mut [u16]) {
        for value in buffer {
            *value = 0;
        }
    }
}

fn enroll_face_template(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<EnrollFaceTemplatePayload>(request.payload.clone())
    {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid face enrollment payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "enroll_face_template",
                }),
            );
        }
    };

    let diagnostics_cli_path = payload.install_dir.join("diagnostics_cli.exe");
    if !diagnostics_cli_path.exists() {
        return SetupResponseEnvelope::failed(
            request,
            format!(
                "Face enrollment failed: diagnostics_cli.exe was not found at {}.",
                diagnostics_cli_path.display()
            ),
            SetupErrorCode::MissingPayloadFile,
        );
    }

    let output_dir = payload.install_dir.join(&payload.output_relative_dir);
    let template_path = payload
        .install_dir
        .join(&payload.output_template_relative_path);
    if let Some(parent) = output_dir.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return SetupResponseEnvelope::failed(
            request,
            format!("Face enrollment output directory preparation failed: {error}"),
            SetupErrorCode::FaceEnrollmentFailed,
        );
    }

    let mut command = Command::new(&diagnostics_cli_path);
    command
        .current_dir(&payload.install_dir)
        .arg("guided-enroll")
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--camera-id")
        .arg(&payload.camera_id)
        .arg("--user-id")
        .arg(&payload.user_id)
        .arg("--accepted-frames-per-step")
        .arg(payload.accepted_frames_per_step.to_string())
        .arg("--max-wait-frames-per-step")
        .arg(payload.max_wait_frames_per_step.to_string())
        .arg("--max-frames-per-step")
        .arg(payload.max_frames_per_step.to_string())
        .arg("--pose-ready-consecutive")
        .arg(payload.pose_ready_consecutive.to_string())
        .arg("--pose-ready-min-fit")
        .arg(payload.pose_ready_min_fit.to_string())
        .arg("--frame-delay-ms")
        .arg(payload.frame_delay_ms.to_string());
    if payload.allow_partial_enrollment {
        command.arg("--allow-partial-enrollment");
    }
    if payload.save_debug_images {
        command.arg("--save-debug-images");
    }

    let command_result = run_packaged_command(command);
    if !command_result.exit_success {
        return SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Failed,
            message: "Face enrollment command failed.".to_owned(),
            safe_details: json!({
                "setup_error_code": SetupErrorCode::FaceEnrollmentFailed,
                "diagnostics_cli_path": diagnostics_cli_path,
                "install_dir": payload.install_dir,
                "enrollment_output_dir": output_dir,
                "exit_code": command_result.exit_code,
                "stdout_tail": command_result.stdout_tail,
                "stderr_tail": command_result.stderr_tail,
            }),
            next_recommended_action: Some(
                "Check camera access, model files, and face enrollment positioning.".to_owned(),
            ),
        };
    }

    let generated_template_path = output_dir.join("selected_templates.json");
    if !generated_template_path.exists() {
        return SetupResponseEnvelope::failed(
            request,
            format!(
                "Face enrollment did not produce {}.",
                generated_template_path.display()
            ),
            SetupErrorCode::FaceEnrollmentFailed,
        );
    }

    if generated_template_path != template_path {
        if let Some(parent) = template_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return SetupResponseEnvelope::failed(
                request,
                format!("Face template target directory preparation failed: {error}"),
                SetupErrorCode::FaceEnrollmentFailed,
            );
        }
        if let Err(error) = fs::copy(&generated_template_path, &template_path) {
            return SetupResponseEnvelope::failed(
                request,
                format!("Face template copy failed: {error}"),
                SetupErrorCode::FaceEnrollmentFailed,
            );
        }
    }

    let template_summary = read_face_template_summary(&template_path).unwrap_or_else(|| {
        json!({
            "selected_template_count": null,
            "rejected_sample_count": null,
        })
    });
    SetupResponseEnvelope::succeeded(
        request,
        "Face template enrolled.",
        json!({
            "install_dir": payload.install_dir,
            "camera_id": payload.camera_id,
            "user_id": payload.user_id,
            "diagnostics_cli_path": diagnostics_cli_path,
            "enrollment_output_dir": output_dir,
            "generated_template_path": generated_template_path,
            "template_path": template_path,
            "template_summary": template_summary,
        }),
    )
}

fn run_auth_self_test(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<RunAuthSelfTestPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid auth self-test payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "run_auth_self_test",
                }),
            );
        }
    };

    let diagnostics_cli_path = payload.install_dir.join("diagnostics_cli.exe");
    if !diagnostics_cli_path.exists() {
        return SetupResponseEnvelope::failed(
            request,
            format!(
                "Auth self-test failed: diagnostics_cli.exe was not found at {}.",
                diagnostics_cli_path.display()
            ),
            SetupErrorCode::MissingPayloadFile,
        );
    }

    let mut command = Command::new(&diagnostics_cli_path);
    command
        .current_dir(&payload.install_dir)
        .arg("service-camera-auth")
        .arg("--session-id")
        .arg(&payload.session_id);

    let command_result = run_packaged_command(command);
    let auth_succeeded = command_result.stdout_tail.contains("AuthSucceeded");
    let credential_ready = command_result.stdout_tail.contains("CredentialReady");
    let self_test_succeeded =
        command_result.exit_success && (!payload.require_credential_ready || credential_ready);

    let safe_details = json!({
        "install_dir": payload.install_dir,
        "diagnostics_cli_path": diagnostics_cli_path,
        "session_id": payload.session_id,
        "auth_succeeded": auth_succeeded,
        "credential_ready": credential_ready,
        "require_credential_ready": payload.require_credential_ready,
        "exit_code": command_result.exit_code,
        "stdout_tail": command_result.stdout_tail,
        "stderr_tail": command_result.stderr_tail,
    });

    if self_test_succeeded {
        SetupResponseEnvelope::succeeded(request, "Auth self-test passed.", safe_details)
    } else {
        SetupResponseEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: request.correlation_id.clone(),
            operation: request.operation,
            operation_status: OperationStatus::Failed,
            message: "Auth self-test failed.".to_owned(),
            safe_details: json!({
                "setup_error_code": SetupErrorCode::AuthSelfTestFailed,
                "details": safe_details,
            }),
            next_recommended_action: Some(
                "Check that the service is running, camera access works, and enrolled credentials exist."
                    .to_owned(),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackagedCommandResult {
    exit_success: bool,
    exit_code: Option<i32>,
    stdout_tail: String,
    stderr_tail: String,
}

fn run_packaged_command(mut command: Command) -> PackagedCommandResult {
    match command.output() {
        Ok(output) => PackagedCommandResult {
            exit_success: output.status.success(),
            exit_code: output.status.code(),
            stdout_tail: safe_utf8_tail(&output.stdout),
            stderr_tail: safe_utf8_tail(&output.stderr),
        },
        Err(error) => PackagedCommandResult {
            exit_success: false,
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: error.to_string(),
        },
    }
}

fn safe_utf8_tail(bytes: &[u8]) -> String {
    const MAX_CHARS: usize = 4000;
    let text = String::from_utf8_lossy(bytes);
    let char_count = text.chars().count();
    if char_count <= MAX_CHARS {
        return text.into_owned();
    }
    text.chars()
        .skip(char_count.saturating_sub(MAX_CHARS))
        .collect()
}

fn read_face_template_summary(template_path: &Path) -> Option<Value> {
    let bytes = fs::read(template_path).ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    let selected_template_count = value
        .get("quality_summary")
        .and_then(|summary| summary.get("selected_template_count"))
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("templates")
                .and_then(Value::as_array)
                .map(|templates| {
                    templates
                        .iter()
                        .filter(|template| {
                            template
                                .get("selected_for_unlock")
                                .and_then(Value::as_bool)
                                .unwrap_or(true)
                        })
                        .count() as u64
                })
        });
    let rejected_sample_count = value
        .get("quality_summary")
        .and_then(|summary| summary.get("rejected_sample_count"))
        .and_then(Value::as_u64);
    Some(json!({
        "selected_template_count": selected_template_count,
        "rejected_sample_count": rejected_sample_count,
    }))
}

fn configure_presence_lock(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload =
        match serde_json::from_value::<ConfigurePresenceLockPayload>(request.payload.clone()) {
            Ok(payload) => payload,
            Err(error) => {
                return SetupResponseEnvelope::invalid_request(
                    request,
                    format!("Invalid configure presence lock payload: {error}"),
                    json!({
                        "setup_error_code": SetupErrorCode::InvalidRequest,
                        "payload_type": "configure_presence_lock",
                    }),
                );
            }
        };

    let patch = match crate::setup_presence::build_presence_patch(&payload) {
        Ok(patch) => patch,
        Err(error) => {
            let setup_error_code = if error.is_invalid_install_dir() {
                SetupErrorCode::InvalidInstallDir
            } else {
                SetupErrorCode::InvalidRequest
            };
            return SetupResponseEnvelope::failed(
                request,
                format!("Configure presence lock plan failed: {error}"),
                setup_error_code,
            );
        }
    };

    let safe_details = json!({
        "presence_lock_enabled": patch.presence_lock_enabled,
        "presence_owner_match_threshold": patch.presence_owner_match_threshold,
        "presence_detector_kind": patch.presence_detector_kind,
        "presence_tracking_mode": patch.presence_tracking_mode,
        "presence_detector_fps": patch.presence_detector_fps,
        "presence_unload_model_when_idle": patch.presence_unload_model_when_idle,
        "presence_person_confidence_threshold": patch.presence_person_confidence_threshold,
        "presence_person_detector_model": patch.presence_person_detector_model,
        "presence_person_suspect_fps": patch.presence_person_suspect_fps,
        "presence_absent_required_frames": patch.presence_absent_required_frames,
        "presence_boundary_margin_ratio": patch.presence_boundary_margin_ratio,
        "presence_movement_delta_ratio": patch.presence_movement_delta_ratio,
        "presence_person_model_path": patch.presence_person_model_path,
        "presence_person_model_config_path": patch.presence_person_model_config_path,
        "presence_person_debug_output_dir": patch.presence_person_debug_output_dir,
    });

    match ServiceAuthRegistry::configure_presence_lock(&patch) {
        Ok(()) => SetupResponseEnvelope::succeeded(
            request,
            "Presence lock configuration updated.",
            safe_details,
        ),
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("Configure presence lock failed: {error}"),
            SetupErrorCode::PresenceConfigFailed,
        ),
    }
}

fn emergency_disable(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    match InstallerOrchestrator::emergency_disable_provider() {
        Ok(()) => SetupResponseEnvelope::succeeded(
            request,
            "Credential Provider enumeration disabled.",
            json!({
                "credential_provider_disabled": true,
            }),
        ),
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("Emergency disable failed: {error}"),
            SetupErrorCode::EmergencyDisableFailed,
        ),
    }
}

fn uninstall(request: &SetupRequestEnvelope) -> SetupResponseEnvelope {
    let payload = match serde_json::from_value::<UninstallPayload>(request.payload.clone()) {
        Ok(payload) => payload,
        Err(error) => {
            return SetupResponseEnvelope::invalid_request(
                request,
                format!("Invalid uninstall payload: {error}"),
                json!({
                    "setup_error_code": SetupErrorCode::InvalidRequest,
                    "payload_type": "uninstall",
                }),
            );
        }
    };
    let plan = FullUninstallPlan {
        resource_plan: ResourceDirectoryPlan::from_environment_or_default(),
        stop_service_first: payload.stop_service_first,
        delete_data: !payload.preserve_data,
    };

    match InstallerOrchestrator::uninstall(&plan) {
        Ok(()) => SetupResponseEnvelope::succeeded(
            request,
            "WinFaceUnlock uninstalled.",
            json!({
                "preserve_data": payload.preserve_data,
                "stop_service_first": payload.stop_service_first,
            }),
        ),
        Err(error) => SetupResponseEnvelope::failed(
            request,
            format!("Uninstall failed: {error}"),
            SetupErrorCode::UninstallFailed,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_enrollment_does_not_accept_json_password_transport_in_unit_tests() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-credential".to_owned(),
            operation: SetupOperation::EnrollCredential,
            payload: json!({
                "username": "Leo16"
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(response.operation, SetupOperation::EnrollCredential);
        assert_eq!(
            response.safe_details["details"]["password_transport"],
            json!("native_windows_credential_prompt")
        );
    }

    #[test]
    fn invalid_protocol_returns_protocol_failure() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION + 1,
            correlation_id: "corr-protocol".to_owned(),
            operation: SetupOperation::GetStatus,
            payload: json!({}),
        };

        let response = handle_request(&request);

        assert_eq!(
            response.operation_status,
            OperationStatus::UnsupportedProtocol
        );
        assert_eq!(
            response.safe_details["supported_protocol_version"],
            json!(SETUP_PROTOCOL_VERSION)
        );
    }

    #[test]
    fn invalid_uninstall_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-payload".to_owned(),
            operation: SetupOperation::Uninstall,
            payload: json!({
                "preserve_data": "yes"
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("invalid_request")
        );
        assert_eq!(response.safe_details["payload_type"], json!("uninstall"));
    }

    #[test]
    fn invalid_preflight_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-preflight".to_owned(),
            operation: SetupOperation::RunPreflight,
            payload: json!({
                "require_elevation": false
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("invalid_request")
        );
        assert_eq!(
            response.safe_details["payload_type"],
            json!("run_preflight")
        );
    }

    #[test]
    fn preflight_missing_payload_file_returns_failed_check() {
        let missing_payload_path = std::env::temp_dir().join("winfaceunlock-missing-payload.exe");
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-preflight-missing-payload".to_owned(),
            operation: SetupOperation::RunPreflight,
            payload: json!({
                "install_dir": std::env::temp_dir(),
                "require_elevation": false,
                "required_payload_files": [
                    {
                        "file_id": "win_service",
                        "path": missing_payload_path
                    }
                ]
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert!(
            response.safe_details["failed_check_ids"]
                .as_array()
                .is_some_and(|checks| checks.contains(&json!("payload_complete")))
        );
    }

    #[test]
    fn inspect_payload_returns_stage_file_list_from_manifest() -> Result<(), std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-inspect-payload-{}",
            std::process::id()
        ));
        let payload_root_dir = root.join("payload");
        std::fs::create_dir_all(payload_root_dir.join("models"))?;
        std::fs::write(payload_root_dir.join("win_service.exe"), b"service")?;
        std::fs::write(
            payload_root_dir.join(r"models\face_detection_yunet_2023mar.onnx"),
            b"yunet",
        )?;
        std::fs::write(
            payload_root_dir.join("winfaceunlock-payload.json"),
            serde_json::to_vec(&json!({
                "manifest_version": setup_api::SETUP_PAYLOAD_MANIFEST_VERSION,
                "payload_files": [
                    {
                        "file_id": "win_service",
                        "source_relative_path": "win_service.exe"
                    },
                    {
                        "file_id": "yunet_model",
                        "source_relative_path": "models\\face_detection_yunet_2023mar.onnx"
                    },
                    {
                        "file_id": "optional_yolox_model",
                        "source_relative_path": "models\\yolox_nano.onnx",
                        "required": false
                    }
                ]
            }))?,
        )?;

        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-inspect-payload".to_owned(),
            operation: SetupOperation::InspectPayload,
            payload: json!({
                "payload_root_dir": payload_root_dir,
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Succeeded);
        assert_eq!(
            response.safe_details["stage_payload_files"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            response.safe_details["inspected_files"][2]["payload_file_presence_status"],
            json!("missing_optional")
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn inspect_payload_missing_required_file_returns_missing_payload_failure()
    -> Result<(), std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-inspect-payload-missing-{}",
            std::process::id()
        ));
        let payload_root_dir = root.join("payload");
        std::fs::create_dir_all(&payload_root_dir)?;
        std::fs::write(
            payload_root_dir.join("winfaceunlock-payload.json"),
            serde_json::to_vec(&json!({
                "manifest_version": setup_api::SETUP_PAYLOAD_MANIFEST_VERSION,
                "payload_files": [
                    {
                        "file_id": "win_service",
                        "source_relative_path": "win_service.exe"
                    }
                ]
            }))?,
        )?;

        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-inspect-payload-missing".to_owned(),
            operation: SetupOperation::InspectPayload,
            payload: json!({
                "payload_root_dir": payload_root_dir,
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("missing_payload_file")
        );
        assert_eq!(
            response.safe_details["missing_required_payload_files"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn invalid_stage_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-stage".to_owned(),
            operation: SetupOperation::StagePayload,
            payload: json!({
                "install_dir": std::env::temp_dir()
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["payload_type"],
            json!("stage_payload")
        );
    }

    #[test]
    fn stage_payload_copies_file_through_backend() -> Result<(), std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-stage-backend-{}",
            std::process::id()
        ));
        let source_dir = root.join("source");
        let install_dir = root.join("install");
        std::fs::create_dir_all(&source_dir)?;
        let source_path = source_dir.join("win_service.exe");
        std::fs::write(&source_path, b"service")?;

        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-stage".to_owned(),
            operation: SetupOperation::StagePayload,
            payload: json!({
                "install_dir": install_dir,
                "overwrite_existing": false,
                "payload_files": [
                    {
                        "file_id": "win_service",
                        "source_path": source_path,
                        "target_relative_path": "win_service.exe"
                    }
                ]
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Succeeded);
        assert_eq!(
            response.safe_details["staged_files"][0]["stage_file_status"],
            json!("copied")
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn stage_payload_resolves_relative_source_from_payload_root() -> Result<(), std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-stage-backend-relative-{}",
            std::process::id()
        ));
        let payload_root_dir = root.join("payload");
        let install_dir = root.join("install");
        std::fs::create_dir_all(payload_root_dir.join("models"))?;
        std::fs::write(
            payload_root_dir.join(r"models\face_detection_yunet_2023mar.onnx"),
            b"yunet",
        )?;

        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-stage-relative-source".to_owned(),
            operation: SetupOperation::StagePayload,
            payload: json!({
                "install_dir": install_dir,
                "payload_root_dir": payload_root_dir,
                "overwrite_existing": false,
                "payload_files": [
                    {
                        "file_id": "yunet_model",
                        "source_path": "models\\face_detection_yunet_2023mar.onnx",
                        "target_relative_path": "models\\face_detection_yunet_2023mar.onnx"
                    }
                ]
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Succeeded);
        assert_eq!(
            response.safe_details["staged_files"][0]["stage_file_status"],
            json!("copied")
        );
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn stage_payload_relative_source_without_payload_root_is_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-stage-relative-source-invalid".to_owned(),
            operation: SetupOperation::StagePayload,
            payload: json!({
                "install_dir": std::env::temp_dir(),
                "payload_files": [
                    {
                        "file_id": "yunet_model",
                        "source_path": "models\\face_detection_yunet_2023mar.onnx",
                        "target_relative_path": "models\\face_detection_yunet_2023mar.onnx"
                    }
                ]
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("invalid_request")
        );
    }

    #[test]
    fn invalid_install_components_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-install-components".to_owned(),
            operation: SetupOperation::InstallSystemComponents,
            payload: json!({
                "start_service": true
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["payload_type"],
            json!("install_system_components")
        );
    }

    #[test]
    fn install_components_missing_staged_files_returns_missing_payload_failure() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-missing-install-components".to_owned(),
            operation: SetupOperation::InstallSystemComponents,
            payload: json!({
                "install_dir": std::env::temp_dir().join("winfaceunlock-missing-install-components")
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("missing_payload_file")
        );
    }

    #[test]
    fn invalid_repair_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-repair".to_owned(),
            operation: SetupOperation::Repair,
            payload: json!({
                "start_service": true
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(response.safe_details["payload_type"], json!("repair"));
    }

    #[test]
    fn repair_missing_staged_files_returns_missing_payload_failure() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-missing-repair-components".to_owned(),
            operation: SetupOperation::Repair,
            payload: json!({
                "install_dir": std::env::temp_dir().join("winfaceunlock-missing-repair-components")
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("missing_payload_file")
        );
    }

    #[test]
    fn invalid_presence_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-presence".to_owned(),
            operation: SetupOperation::ConfigurePresenceLock,
            payload: json!({
                "presence_lock_enabled": "enabled"
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["payload_type"],
            json!("configure_presence_lock")
        );
    }

    #[test]
    fn presence_model_path_without_install_dir_returns_invalid_install_dir() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-presence-missing-install-dir".to_owned(),
            operation: SetupOperation::ConfigurePresenceLock,
            payload: json!({
                "person_model_relative_path": "models\\yolov8n.onnx"
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("invalid_install_dir")
        );
    }

    #[test]
    fn invalid_face_enrollment_payload_returns_invalid_request() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-invalid-face-enroll".to_owned(),
            operation: SetupOperation::EnrollFaceTemplate,
            payload: json!({
                "camera_id": "opencv-index:0"
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::InvalidRequest);
        assert_eq!(
            response.safe_details["payload_type"],
            json!("enroll_face_template")
        );
    }

    #[test]
    fn face_enrollment_missing_diagnostics_cli_returns_missing_payload_failure() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-face-enroll-missing-diag".to_owned(),
            operation: SetupOperation::EnrollFaceTemplate,
            payload: json!({
                "install_dir": std::env::temp_dir().join("winfaceunlock-missing-diag"),
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("missing_payload_file")
        );
    }

    #[test]
    fn auth_self_test_missing_diagnostics_cli_returns_missing_payload_failure() {
        let request = SetupRequestEnvelope {
            protocol_version: SETUP_PROTOCOL_VERSION,
            correlation_id: "corr-auth-self-test-missing-diag".to_owned(),
            operation: SetupOperation::RunAuthSelfTest,
            payload: json!({
                "install_dir": std::env::temp_dir().join("winfaceunlock-missing-auth-diag"),
            }),
        };

        let response = handle_request(&request);

        assert_eq!(response.operation_status, OperationStatus::Failed);
        assert_eq!(
            response.safe_details["setup_error_code"],
            json!("missing_payload_file")
        );
    }

    #[test]
    fn face_template_summary_counts_selected_templates() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-template-summary-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root)?;
        let template_path = root.join("selected_templates.json");
        std::fs::write(
            &template_path,
            serde_json::to_vec(&json!({
                "templates": [
                    { "selected_for_unlock": true },
                    { "selected_for_unlock": false },
                    {}
                ]
            }))?,
        )?;

        let summary = read_face_template_summary(&template_path).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "template summary should parse selected_templates.json",
            )
        })?;
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(summary["selected_template_count"], json!(2));
        assert_eq!(summary["rejected_sample_count"], Value::Null);
        Ok(())
    }
}
