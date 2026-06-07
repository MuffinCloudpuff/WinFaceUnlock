#![allow(unsafe_code)]

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use common_protocol::SERVICE_NAME;
use control_protocol::{
    DashboardStatus, DataDirectorySummary, PathPresence, PresenceMonitorState,
    PresenceRuntimeSummary, ProviderRegistrationState, ProviderStatusSummary, RegistryConfigState,
    ServiceConfigSummary, ServiceInstallationState, ServiceRuntimeState, ServiceStatusSummary,
};
use serde::Deserialize;
use windows_provider::{
    COM_INPROC_SERVER_REGISTRY_PATH, PROVIDER_CLSID_REGISTRY_PATH, PROVIDER_ROOT_REGISTRY_PATH,
};
use windows_service::{
    service::{ServiceAccess, ServiceState, ServiceStatus},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const APP_DATA_DIR_NAME: &str = "WinFaceUnlock";
const PRESENCE_AUDIT_DIR_NAME: &str = "presence-audit";
const PRESENCE_RUNTIME_STATUS_FILE_NAME: &str = "presence-runtime-status.json";
const SERVICE_CONFIG_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\Service";
const REG_AUTH_MODE: &str = "AuthMode";
const REG_FACE_TEMPLATE_PATH: &str = "FaceTemplatePath";
const REG_PRESENCE_LOCK_ENABLED: &str = "PresenceLockEnabled";
const REG_PRESENCE_DETECTOR_KIND: &str = "PresenceDetectorKind";
const REG_PRESENCE_TRACKING_MODE: &str = "PresenceTrackingMode";
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlStatusPaths {
    pub program_data_dir: PathBuf,
    pub presence_audit_dir: PathBuf,
    pub presence_runtime_status_path: PathBuf,
}

impl ControlStatusPaths {
    pub fn from_environment_or_default() -> Self {
        let program_data_dir = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_DATA_DIR_NAME);
        Self::from_program_data_dir(program_data_dir)
    }

    pub fn from_program_data_dir(program_data_dir: PathBuf) -> Self {
        Self {
            presence_audit_dir: program_data_dir.join(PRESENCE_AUDIT_DIR_NAME),
            presence_runtime_status_path: program_data_dir.join(PRESENCE_RUNTIME_STATUS_FILE_NAME),
            program_data_dir,
        }
    }
}

pub struct WindowsDashboardStatusProvider {
    paths: ControlStatusPaths,
}

impl WindowsDashboardStatusProvider {
    pub fn from_environment_or_default() -> Self {
        Self {
            paths: ControlStatusPaths::from_environment_or_default(),
        }
    }

    pub fn with_paths(paths: ControlStatusPaths) -> Self {
        Self { paths }
    }

    pub fn load_dashboard_status(&self) -> Result<DashboardStatus, ControlStatusError> {
        load_dashboard_status(&WindowsStatusSource { paths: &self.paths })
    }
}

impl Default for WindowsDashboardStatusProvider {
    fn default() -> Self {
        Self::from_environment_or_default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlStatusError {
    ServiceStatusUnavailable(String),
    ProviderStatusUnavailable(String),
    ServiceConfigUnavailable(String),
    DataDirectoryStatusUnavailable(String),
    PresenceRuntimeStatusUnavailable(String),
    PermissionDenied(String),
}

impl ControlStatusError {
    pub fn message(&self) -> &str {
        match self {
            Self::ServiceStatusUnavailable(message)
            | Self::ProviderStatusUnavailable(message)
            | Self::ServiceConfigUnavailable(message)
            | Self::DataDirectoryStatusUnavailable(message)
            | Self::PresenceRuntimeStatusUnavailable(message)
            | Self::PermissionDenied(message) => message,
        }
    }
}

impl fmt::Display for ControlStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ControlStatusError {}

trait StatusSource {
    fn query_service_status(&self) -> Result<Option<ObservedServiceStatus>, ControlStatusError>;
    fn query_provider_registry_status(
        &self,
    ) -> Result<ObservedProviderRegistryStatus, ControlStatusError>;
    fn query_service_config_status(
        &self,
    ) -> Result<ObservedServiceConfigStatus, ControlStatusError>;
    fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError>;
    fn query_presence_runtime_status(
        &self,
    ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedServiceStatus {
    runtime_state: ServiceRuntimeState,
    process_id: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedProviderRegistryStatus {
    credential_provider_registered: bool,
    com_server_registered: bool,
    project_config_registered: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedServiceConfigStatus {
    registry_config_exists: bool,
    auth_mode: Option<String>,
    face_template_path: Option<String>,
    presence_lock_enabled: Option<String>,
    presence_detector_kind: Option<String>,
    presence_tracking_mode: Option<String>,
}

fn load_dashboard_status(
    source: &impl StatusSource,
) -> Result<DashboardStatus, ControlStatusError> {
    Ok(DashboardStatus {
        service: map_service_status(source.query_service_status()?),
        provider: map_provider_status(source.query_provider_registry_status()?),
        service_config: map_service_config(source.query_service_config_status()?),
        data_directory: source.query_data_directory_status()?,
        presence_runtime: source.query_presence_runtime_status()?,
    })
}

fn map_service_status(status: Option<ObservedServiceStatus>) -> ServiceStatusSummary {
    match status {
        Some(status) => ServiceStatusSummary {
            installation_state: ServiceInstallationState::Installed,
            runtime_state: status.runtime_state,
            process_id: status.process_id,
        },
        None => ServiceStatusSummary {
            installation_state: ServiceInstallationState::Missing,
            runtime_state: ServiceRuntimeState::Missing,
            process_id: None,
        },
    }
}

fn map_provider_status(status: ObservedProviderRegistryStatus) -> ProviderStatusSummary {
    let registration_state = if status.credential_provider_registered
        && status.com_server_registered
        && status.project_config_registered
    {
        ProviderRegistrationState::Registered
    } else if status.credential_provider_registered
        || status.com_server_registered
        || status.project_config_registered
    {
        ProviderRegistrationState::PartiallyRegistered
    } else {
        ProviderRegistrationState::NotRegistered
    };

    ProviderStatusSummary {
        registration_state,
        credential_provider_registered: status.credential_provider_registered,
        com_server_registered: status.com_server_registered,
        project_config_registered: status.project_config_registered,
    }
}

fn map_service_config(status: ObservedServiceConfigStatus) -> ServiceConfigSummary {
    ServiceConfigSummary {
        registry_config_state: if status.registry_config_exists {
            RegistryConfigState::Present
        } else {
            RegistryConfigState::Missing
        },
        auth_mode: status.auth_mode,
        face_template_path: status.face_template_path,
        presence_lock_enabled: status
            .presence_lock_enabled
            .as_deref()
            .and_then(parse_registry_bool),
        presence_detector_kind: status.presence_detector_kind,
        presence_tracking_mode: status.presence_tracking_mode,
    }
}

fn parse_registry_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "enabled" => Some(true),
        "0" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

struct WindowsStatusSource<'a> {
    paths: &'a ControlStatusPaths,
}

impl StatusSource for WindowsStatusSource<'_> {
    fn query_service_status(&self) -> Result<Option<ObservedServiceStatus>, ControlStatusError> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(service_error)?;
        match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(service) => service
                .query_status()
                .map(|status| Some(map_observed_service_status(status)))
                .map_err(service_error),
            Err(error) if is_service_missing_error(&error) => Ok(None),
            Err(error) => Err(service_error(error)),
        }
    }

    fn query_provider_registry_status(
        &self,
    ) -> Result<ObservedProviderRegistryStatus, ControlStatusError> {
        Ok(ObservedProviderRegistryStatus {
            credential_provider_registered: registry::key_exists(PROVIDER_CLSID_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
            com_server_registered: registry::key_exists(COM_INPROC_SERVER_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
            project_config_registered: registry::key_exists(PROVIDER_ROOT_REGISTRY_PATH)
                .map_err(provider_registry_error)?,
        })
    }

    fn query_service_config_status(
        &self,
    ) -> Result<ObservedServiceConfigStatus, ControlStatusError> {
        let registry_config_exists =
            registry::key_exists(SERVICE_CONFIG_REGISTRY_PATH).map_err(service_config_error)?;
        Ok(ObservedServiceConfigStatus {
            registry_config_exists,
            auth_mode: registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_AUTH_MODE)
                .map_err(service_config_error)?,
            face_template_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_FACE_TEMPLATE_PATH,
            )
            .map_err(service_config_error)?,
            presence_lock_enabled: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_LOCK_ENABLED,
            )
            .map_err(service_config_error)?,
            presence_detector_kind: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_DETECTOR_KIND,
            )
            .map_err(service_config_error)?,
            presence_tracking_mode: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_TRACKING_MODE,
            )
            .map_err(service_config_error)?,
        })
    }

    fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError> {
        Ok(DataDirectorySummary {
            program_data_dir: Some(self.paths.program_data_dir.display().to_string()),
            program_data_presence: path_presence(&self.paths.program_data_dir),
            presence_audit_dir: Some(self.paths.presence_audit_dir.display().to_string()),
            presence_audit_presence: path_presence(&self.paths.presence_audit_dir),
        })
    }

    fn query_presence_runtime_status(
        &self,
    ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError> {
        if !self.paths.presence_runtime_status_path.exists() {
            return Ok(Some(PresenceRuntimeSummary {
                monitor_state: PresenceMonitorState::Unavailable,
                session_id: None,
                reason: Some("presence runtime status file is missing".to_owned()),
                updated_at_unix_ms: None,
            }));
        }
        let text =
            fs::read_to_string(&self.paths.presence_runtime_status_path).map_err(|error| {
                ControlStatusError::PresenceRuntimeStatusUnavailable(format!(
                    "presence runtime status file cannot be read: {error}"
                ))
            })?;
        let status: PresenceRuntimeStatusFile = serde_json::from_str(&text).map_err(|error| {
            ControlStatusError::PresenceRuntimeStatusUnavailable(format!(
                "presence runtime status file is invalid: {error}"
            ))
        })?;
        Ok(Some(status.into_summary()))
    }
}

fn map_observed_service_status(status: ServiceStatus) -> ObservedServiceStatus {
    ObservedServiceStatus {
        runtime_state: map_service_runtime_state(status.current_state),
        process_id: status.process_id,
    }
}

fn map_service_runtime_state(state: ServiceState) -> ServiceRuntimeState {
    match state {
        ServiceState::Running => ServiceRuntimeState::Running,
        ServiceState::Stopped => ServiceRuntimeState::Stopped,
        ServiceState::Paused => ServiceRuntimeState::Paused,
        ServiceState::StartPending => ServiceRuntimeState::StartPending,
        ServiceState::StopPending => ServiceRuntimeState::StopPending,
        other => ServiceRuntimeState::Unknown(format!("{other:?}")),
    }
}

fn path_presence(path: &Path) -> PathPresence {
    if path.exists() {
        PathPresence::Present
    } else {
        PathPresence::Missing
    }
}

#[derive(Deserialize)]
struct PresenceRuntimeStatusFile {
    state: String,
    session_id: Option<u32>,
    reason: String,
    updated_at_unix_ms: i64,
}

impl PresenceRuntimeStatusFile {
    fn into_summary(self) -> PresenceRuntimeSummary {
        PresenceRuntimeSummary {
            monitor_state: map_presence_monitor_state(&self.state),
            session_id: self.session_id,
            reason: Some(self.reason),
            updated_at_unix_ms: Some(self.updated_at_unix_ms),
        }
    }
}

fn map_presence_monitor_state(state: &str) -> PresenceMonitorState {
    match state {
        "running" => PresenceMonitorState::Running,
        "stopped" => PresenceMonitorState::Stopped,
        "disabled" => PresenceMonitorState::Disabled,
        "unavailable" => PresenceMonitorState::Unavailable,
        other => PresenceMonitorState::Unknown(other.to_owned()),
    }
}

fn service_error(error: windows_service::Error) -> ControlStatusError {
    if is_access_denied_service_error(&error) {
        ControlStatusError::PermissionDenied("service status access denied".to_owned())
    } else {
        ControlStatusError::ServiceStatusUnavailable(format!(
            "service status cannot be queried: {error}"
        ))
    }
}

fn is_service_missing_error(error: &windows_service::Error) -> bool {
    matches!(
        error,
        windows_service::Error::Winapi(io_error)
            if io_error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST)
    )
}

fn is_access_denied_service_error(error: &windows_service::Error) -> bool {
    matches!(
        error,
        windows_service::Error::Winapi(io_error)
            if io_error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
    )
}

fn provider_registry_error(error: RegistryReadError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::PermissionDenied("provider registry access denied".to_owned())
    } else {
        ControlStatusError::ProviderStatusUnavailable(format!(
            "provider registry status cannot be read: {error}"
        ))
    }
}

fn service_config_error(error: RegistryReadError) -> ControlStatusError {
    if error.is_access_denied() {
        ControlStatusError::PermissionDenied("service config registry access denied".to_owned())
    } else {
        ControlStatusError::ServiceConfigUnavailable(format!(
            "service config registry status cannot be read: {error}"
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegistryReadError {
    operation: &'static str,
    path: String,
    value_name: Option<String>,
    code: u32,
}

impl RegistryReadError {
    fn is_access_denied(&self) -> bool {
        self.code == ERROR_ACCESS_DENIED
    }
}

impl fmt::Display for RegistryReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value_name {
            Some(value_name) => write!(
                formatter,
                "{} failed for {}\\{}: error {}",
                self.operation, self.path, value_name, self.code
            ),
            None => write!(
                formatter,
                "{} failed for {}: error {}",
                self.operation, self.path, self.code
            ),
        }
    }
}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        },
    };

    use super::RegistryReadError;

    pub fn key_exists(path: &str) -> Result<bool, RegistryReadError> {
        match open_key(path) {
            Ok(Some(_key)) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn read_string_value(
        path: &str,
        value_name: &str,
    ) -> Result<Option<String>, RegistryReadError> {
        let Some(key) = open_key(path)? else {
            return Ok(None);
        };
        let name = to_wide_null(value_name);
        let mut data_type = 0_u32;
        let mut byte_len = 0_u32;
        let probe_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                ptr::null_mut(),
                &mut byte_len,
            )
        };
        if probe_status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if probe_status != ERROR_SUCCESS {
            return Err(registry_error(
                "query value",
                path,
                Some(value_name),
                probe_status,
            ));
        }
        if data_type != REG_SZ || byte_len < 2 {
            return Ok(None);
        }

        let mut data = vec![0_u16; byte_len as usize / size_of::<u16>()];
        let query_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                data.as_mut_ptr().cast::<u8>(),
                &mut byte_len,
            )
        };
        if query_status != ERROR_SUCCESS {
            return Err(registry_error(
                "query value",
                path,
                Some(value_name),
                query_status,
            ));
        }
        if data_type != REG_SZ {
            return Ok(None);
        }
        if let Some(terminator_index) = data.iter().position(|value| *value == 0) {
            data.truncate(terminator_index);
        }
        Ok(String::from_utf16(&data).ok())
    }

    fn open_key(path: &str) -> Result<Option<OwnedRegistryKey>, RegistryReadError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Some(OwnedRegistryKey { raw: key }))
        } else if status == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(registry_error("open key", path, None, status))
        }
    }

    struct OwnedRegistryKey {
        raw: HKEY,
    }

    impl Drop for OwnedRegistryKey {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    let _ = RegCloseKey(self.raw);
                }
            }
        }
    }

    fn registry_error(
        operation: &'static str,
        path: &str,
        value_name: Option<&str>,
        code: WIN32_ERROR,
    ) -> RegistryReadError {
        RegistryReadError {
            operation,
            path: path.to_owned(),
            value_name: value_name.map(str::to_owned),
            code,
        }
    }

    fn to_wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod registry {
    use super::RegistryReadError;

    pub fn key_exists(_path: &str) -> Result<bool, RegistryReadError> {
        Ok(false)
    }

    pub fn read_string_value(
        _path: &str,
        _value_name: &str,
    ) -> Result<Option<String>, RegistryReadError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Clone)]
    struct FakeStatusSource {
        service: Result<Option<ObservedServiceStatus>, ControlStatusError>,
        provider: Result<ObservedProviderRegistryStatus, ControlStatusError>,
        service_config: Result<ObservedServiceConfigStatus, ControlStatusError>,
        data_directory: Result<DataDirectorySummary, ControlStatusError>,
        presence_runtime: Result<Option<PresenceRuntimeSummary>, ControlStatusError>,
    }

    impl Default for FakeStatusSource {
        fn default() -> Self {
            Self {
                service: Ok(Some(ObservedServiceStatus {
                    runtime_state: ServiceRuntimeState::Running,
                    process_id: Some(42),
                })),
                provider: Ok(ObservedProviderRegistryStatus {
                    credential_provider_registered: true,
                    com_server_registered: true,
                    project_config_registered: true,
                }),
                service_config: Ok(ObservedServiceConfigStatus {
                    registry_config_exists: true,
                    auth_mode: Some("local-camera".to_owned()),
                    face_template_path: Some(r"C:\ProgramData\WinFaceUnlock\faces.json".to_owned()),
                    presence_lock_enabled: Some("true".to_owned()),
                    presence_detector_kind: Some("person".to_owned()),
                    presence_tracking_mode: Some("owner-face".to_owned()),
                }),
                data_directory: Ok(DataDirectorySummary {
                    program_data_dir: Some(r"C:\ProgramData\WinFaceUnlock".to_owned()),
                    program_data_presence: PathPresence::Present,
                    presence_audit_dir: Some(
                        r"C:\ProgramData\WinFaceUnlock\presence-audit".to_owned(),
                    ),
                    presence_audit_presence: PathPresence::Present,
                }),
                presence_runtime: Ok(Some(PresenceRuntimeSummary {
                    monitor_state: PresenceMonitorState::Running,
                    session_id: Some(1),
                    reason: Some("monitor_running".to_owned()),
                    updated_at_unix_ms: Some(1_782_000_000_000),
                })),
            }
        }
    }

    impl StatusSource for FakeStatusSource {
        fn query_service_status(
            &self,
        ) -> Result<Option<ObservedServiceStatus>, ControlStatusError> {
            self.service.clone()
        }

        fn query_provider_registry_status(
            &self,
        ) -> Result<ObservedProviderRegistryStatus, ControlStatusError> {
            self.provider.clone()
        }

        fn query_service_config_status(
            &self,
        ) -> Result<ObservedServiceConfigStatus, ControlStatusError> {
            self.service_config.clone()
        }

        fn query_data_directory_status(&self) -> Result<DataDirectorySummary, ControlStatusError> {
            self.data_directory.clone()
        }

        fn query_presence_runtime_status(
            &self,
        ) -> Result<Option<PresenceRuntimeSummary>, ControlStatusError> {
            self.presence_runtime.clone()
        }
    }

    #[test]
    fn service_missing_maps_to_distinct_service_states() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            service: Ok(None),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.service.installation_state,
            ServiceInstallationState::Missing
        );
        assert_eq!(status.service.runtime_state, ServiceRuntimeState::Missing);
        assert_eq!(status.service.process_id, None);
        Ok(())
    }

    #[test]
    fn provider_partially_registered_keeps_layer_details() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            provider: Ok(ObservedProviderRegistryStatus {
                credential_provider_registered: true,
                com_server_registered: false,
                project_config_registered: true,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.provider.registration_state,
            ProviderRegistrationState::PartiallyRegistered
        );
        assert!(status.provider.credential_provider_registered);
        assert!(!status.provider.com_server_registered);
        assert!(status.provider.project_config_registered);
        Ok(())
    }

    #[test]
    fn registry_config_missing_maps_without_losing_provider_state() -> Result<(), ControlStatusError>
    {
        let source = FakeStatusSource {
            service_config: Ok(ObservedServiceConfigStatus {
                registry_config_exists: false,
                auth_mode: None,
                face_template_path: None,
                presence_lock_enabled: None,
                presence_detector_kind: None,
                presence_tracking_mode: None,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.service_config.registry_config_state,
            RegistryConfigState::Missing
        );
        assert_eq!(status.service_config.auth_mode, None);
        assert_eq!(
            status.provider.registration_state,
            ProviderRegistrationState::Registered
        );
        Ok(())
    }

    #[test]
    fn data_directory_missing_keeps_both_path_presence_values() -> Result<(), ControlStatusError> {
        let source = FakeStatusSource {
            data_directory: Ok(DataDirectorySummary {
                program_data_dir: Some(r"C:\ProgramData\WinFaceUnlock".to_owned()),
                program_data_presence: PathPresence::Missing,
                presence_audit_dir: Some(r"C:\ProgramData\WinFaceUnlock\presence-audit".to_owned()),
                presence_audit_presence: PathPresence::Missing,
            }),
            ..FakeStatusSource::default()
        };

        let status = load_dashboard_status(&source)?;

        assert_eq!(
            status.data_directory.program_data_presence,
            PathPresence::Missing
        );
        assert_eq!(
            status.data_directory.presence_audit_presence,
            PathPresence::Missing
        );
        Ok(())
    }

    #[test]
    fn provider_status_failure_returns_provider_specific_error() -> Result<(), &'static str> {
        let source = FakeStatusSource {
            provider: Err(ControlStatusError::ProviderStatusUnavailable(
                "provider registry status cannot be read".to_owned(),
            )),
            ..FakeStatusSource::default()
        };

        let error = match load_dashboard_status(&source) {
            Ok(_) => {
                return Err("provider failure should surface");
            }
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ControlStatusError::ProviderStatusUnavailable(_)
        ));
        assert!(error.message().contains("provider registry"));
        Ok(())
    }

    #[test]
    fn presence_runtime_file_state_maps_to_protocol_state() {
        let status = PresenceRuntimeStatusFile {
            state: "disabled".to_owned(),
            session_id: None,
            reason: "presence_lock_disabled".to_owned(),
            updated_at_unix_ms: 100,
        }
        .into_summary();

        assert_eq!(status.monitor_state, PresenceMonitorState::Disabled);
        assert_eq!(status.reason.as_deref(), Some("presence_lock_disabled"));
    }

    #[test]
    fn bool_registry_values_accept_setup_and_windows_spellings() {
        let values = VecDeque::from(["true", "1", "enabled", "false", "0", "disabled"]);

        assert_eq!(
            values
                .iter()
                .map(|value| parse_registry_bool(value))
                .collect::<Vec<_>>(),
            vec![
                Some(true),
                Some(true),
                Some(true),
                Some(false),
                Some(false),
                Some(false)
            ]
        );
    }
}
