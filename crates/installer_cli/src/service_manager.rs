use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

use common_protocol::SERVICE_NAME;

use crate::{
    provider_registry::ProviderRegistryError, resource_directory::ResourceDirectoryError,
    service_registry::ServiceRegistryError, user_startup::UserStartupError,
};
use windows_service::{
    service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl,
        ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType,
        ServiceState, ServiceStatus, ServiceType,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const SERVICE_DISPLAY_NAME: &str = "WinFaceUnlock Service";
const SERVICE_DESCRIPTION: &str = "WinFaceUnlock local authentication and unlock broker service.";
const SERVICE_OPERATION_TIMEOUT: Duration = Duration::from_secs(20);
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(300);
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInstallPlan {
    pub service_name: OsString,
    pub display_name: OsString,
    pub service_binary_path: PathBuf,
}

impl ServiceInstallPlan {
    pub fn new(service_binary_path: PathBuf) -> Self {
        Self {
            service_name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY_NAME),
            service_binary_path,
        }
    }

    fn to_service_info(&self) -> ServiceInfo {
        ServiceInfo {
            name: self.service_name.clone(),
            display_name: self.display_name.clone(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: self.service_binary_path.clone(),
            launch_arguments: vec![OsString::from("--service")],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceLifecycleCommand {
    Start,
    Stop,
}

pub struct ServiceManagerFacade {
    manager: ServiceManager,
}

impl ServiceManagerFacade {
    pub fn connect() -> Result<Self, InstallerError> {
        Self::connect_with_access(ServiceManagerAccess::CONNECT)
    }

    pub fn connect_for_installation() -> Result<Self, InstallerError> {
        Self::connect_with_access(
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
    }

    fn connect_with_access(access: ServiceManagerAccess) -> Result<Self, InstallerError> {
        let manager = ServiceManager::local_computer(None::<&str>, access)?;
        Ok(Self { manager })
    }

    pub fn install_service(&self, plan: &ServiceInstallPlan) -> Result<(), InstallerError> {
        ensure_service_binary_exists(&plan.service_binary_path)?;
        preserve_credential_store_location(&plan.service_binary_path, None)?;
        let service_info = plan.to_service_info();
        let service = self
            .manager
            .create_service(&service_info, service_configuration_access())?;
        configure_service_metadata(&service)
    }

    pub fn repair_service(&self, plan: &ServiceInstallPlan) -> Result<(), InstallerError> {
        ensure_service_binary_exists(&plan.service_binary_path)?;
        let service_info = plan.to_service_info();
        match self
            .manager
            .open_service(SERVICE_NAME, service_configuration_access())
        {
            Ok(service) => {
                let previous_config = service.query_config()?;
                preserve_credential_store_location(
                    &plan.service_binary_path,
                    Some(&previous_config.executable_path),
                )?;
                service.change_config(&service_info)?;
                configure_service_metadata(&service)
            }
            Err(error) if is_service_missing_error(&error) => self.install_service(plan),
            Err(error) => Err(error.into()),
        }
    }

    pub fn uninstall_service(&self, stop_first: bool) -> Result<(), InstallerError> {
        let access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
        let service = self.manager.open_service(SERVICE_NAME, access)?;

        if stop_first && service.query_status()?.current_state != ServiceState::Stopped {
            service.stop()?;
            wait_for_service_state(&service, ServiceState::Stopped)?;
        }
        service.delete()?;
        drop(service);

        wait_for_service_deleted(&self.manager)
    }

    pub fn preserve_existing_credential_store_location(&self) -> Result<(), InstallerError> {
        let service = match self
            .manager
            .open_service(SERVICE_NAME, ServiceAccess::QUERY_CONFIG)
        {
            Ok(service) => service,
            Err(error) if is_service_missing_error(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let command = service.query_config()?.executable_path;
        let binary = service_binary_from_command(&command)?;
        preserve_credential_store_location(&binary, Some(&command))
    }

    pub fn uninstall_service_if_exists(&self, stop_first: bool) -> Result<(), InstallerError> {
        let access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
        let service = match self.manager.open_service(SERVICE_NAME, access) {
            Ok(service) => service,
            Err(error) if is_service_missing_error(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        };

        if stop_first && service.query_status()?.current_state != ServiceState::Stopped {
            service.stop()?;
            wait_for_service_state(&service, ServiceState::Stopped)?;
        }
        service.delete()?;
        drop(service);

        wait_for_service_deleted(&self.manager)
    }

    pub fn run_lifecycle_command(
        &self,
        command: ServiceLifecycleCommand,
    ) -> Result<(), InstallerError> {
        match command {
            ServiceLifecycleCommand::Start => {
                let service = self.manager.open_service(
                    SERVICE_NAME,
                    ServiceAccess::QUERY_STATUS | ServiceAccess::START,
                )?;
                if service.query_status()?.current_state == ServiceState::Running {
                    return Ok(());
                }
                service.start::<&str>(&[])?;
                wait_for_service_state(&service, ServiceState::Running)
            }
            ServiceLifecycleCommand::Stop => {
                let service = self.manager.open_service(
                    SERVICE_NAME,
                    ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
                )?;
                if service.query_status()?.current_state == ServiceState::Stopped {
                    return Ok(());
                }
                service.stop()?;
                wait_for_service_state(&service, ServiceState::Stopped)
            }
        }
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn stop_service_if_exists(&self) -> Result<(), InstallerError> {
        let service = match self.manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
        ) {
            Ok(service) => service,
            Err(error) if is_service_missing_error(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if service.query_status()?.current_state == ServiceState::Stopped {
            return Ok(());
        }
        service.stop()?;
        wait_for_service_state(&service, ServiceState::Stopped)
    }

    pub fn restart_service(&self) -> Result<(), InstallerError> {
        let service = self.manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::START,
        )?;
        if service.query_status()?.current_state != ServiceState::Stopped {
            service.stop()?;
            wait_for_service_state(&service, ServiceState::Stopped)?;
        }
        service.start::<&str>(&[])?;
        wait_for_service_state(&service, ServiceState::Running)
    }

    pub fn query_service_status(&self) -> Result<ServiceStatus, InstallerError> {
        let service = self
            .manager
            .open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
        Ok(service.query_status()?)
    }
}

fn preserve_credential_store_location(
    new_service_binary: &Path,
    previous_service_command: Option<&Path>,
) -> Result<(), InstallerError> {
    use credential_store::location::{
        load_pinned_store_directory, pin_store_directory, validate_store_directory,
    };
    let storage_error = |error| {
        InstallerError::InvalidArguments(format!(
            "credential store location unavailable: {error:?}"
        ))
    };
    if let Some(pinned) = load_pinned_store_directory().map_err(storage_error)? {
        validate_store_directory(&pinned, true).map_err(storage_error)?;
        return Ok(());
    }
    let legacy_override = if previous_service_command.is_some() {
        credential_store::location::legacy_store_directory().map_err(storage_error)?
    } else {
        None
    };
    let directory = match legacy_override {
        Some(directory) => directory,
        None => {
            credential_store_directory_for_install(new_service_binary, previous_service_command)?
        }
    };
    validate_store_directory(&directory, true).map_err(storage_error)?;
    pin_store_directory(&directory).map_err(storage_error)?;
    Ok(())
}

fn credential_store_directory_for_install(
    new_service_binary: &Path,
    previous_service_command: Option<&Path>,
) -> Result<PathBuf, InstallerError> {
    let binary = if let Some(command) = previous_service_command {
        service_binary_from_command(command)?
    } else {
        new_service_binary.to_path_buf()
    };
    if !binary.is_absolute()
        || binary
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(InstallerError::InvalidArguments(
            "service binary path must be absolute".to_owned(),
        ));
    }
    binary
        .parent()
        .map(|path| path.join(credential_store::location::STORE_DIRECTORY_NAME))
        .ok_or_else(|| {
            InstallerError::InvalidArguments("service binary has no parent directory".to_owned())
        })
}

fn service_binary_from_command(command: &Path) -> Result<PathBuf, InstallerError> {
    let invalid = || {
        InstallerError::InvalidArguments(
            "existing service command has no valid absolute executable path".to_owned(),
        )
    };
    let command = command.to_str().ok_or_else(invalid)?.trim();
    let binary = if let Some(quoted) = command.strip_prefix('"') {
        let (binary, arguments) = quoted.split_once('"').ok_or_else(invalid)?;
        if !arguments.is_empty() && !arguments.starts_with(char::is_whitespace) {
            return Err(invalid());
        }
        binary
    } else {
        command.split_whitespace().next().ok_or_else(invalid)?
    };
    let binary = PathBuf::from(binary);
    if !binary.is_absolute()
        || !binary
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err(invalid());
    }
    Ok(binary)
}

fn service_configuration_access() -> ServiceAccess {
    ServiceAccess::QUERY_CONFIG
        | ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
}

fn configure_service_metadata(
    service: &windows_service::service::Service,
) -> Result<(), InstallerError> {
    service.set_description(SERVICE_DESCRIPTION)?;
    service.set_delayed_auto_start(false)?;
    service.update_failure_actions(service_failure_actions())?;
    service.set_failure_actions_on_non_crash_failures(true)?;
    Ok(())
}

fn service_failure_actions() -> ServiceFailureActions {
    ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(5),
            },
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(30),
            },
            ServiceAction {
                // SCM repeats the final action for all subsequent failures.
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(120),
            },
        ]),
    }
}

fn ensure_service_binary_exists(service_binary_path: &Path) -> Result<(), InstallerError> {
    if !service_binary_path.is_file() {
        return Err(InstallerError::InvalidArguments(format!(
            "service binary does not exist: {}",
            service_binary_path.display()
        )));
    }
    let worker = service_binary_path
        .parent()
        .ok_or_else(|| {
            InstallerError::InvalidArguments("service binary has no parent directory".to_owned())
        })?
        .join("camera_worker.exe");
    if !worker.is_file() {
        return Err(InstallerError::InvalidArguments(format!(
            "required camera worker does not exist beside service binary: {}",
            worker.display()
        )));
    }
    Ok(())
}

fn wait_for_service_state(
    service: &windows_service::service::Service,
    expected_state: ServiceState,
) -> Result<(), InstallerError> {
    let start = Instant::now();
    loop {
        let status = service.query_status()?;
        if status.current_state == expected_state {
            return Ok(());
        }
        if start.elapsed() >= SERVICE_OPERATION_TIMEOUT {
            return Err(InstallerError::TimedOut(format!(
                "service did not reach {expected_state:?}, current state: {:?}",
                status.current_state
            )));
        }
        sleep(SERVICE_POLL_INTERVAL);
    }
}

fn wait_for_service_deleted(manager: &ServiceManager) -> Result<(), InstallerError> {
    let start = Instant::now();
    loop {
        match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
            Ok(_) if start.elapsed() >= SERVICE_OPERATION_TIMEOUT => {
                return Err(InstallerError::TimedOut(
                    "service is marked for deletion but still visible".to_owned(),
                ));
            }
            Ok(_) => sleep(SERVICE_POLL_INTERVAL),
            Err(error) if is_service_missing_error(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}

fn is_service_missing_error(error: &windows_service::Error) -> bool {
    matches!(
        error,
        windows_service::Error::Winapi(io_error)
            if io_error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST)
    )
}

#[derive(Debug)]
pub enum InstallerError {
    InvalidArguments(String),
    Io(std::io::Error),
    ProviderRegistry(ProviderRegistryError),
    ResourceDirectory(ResourceDirectoryError),
    ServiceRegistry(ServiceRegistryError),
    Service(windows_service::Error),
    TimedOut(String),
    UserStartup(UserStartupError),
}

impl fmt::Display for InstallerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(message) => write!(formatter, "invalid arguments: {message}"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::ProviderRegistry(error) => write!(formatter, "provider registry error: {error}"),
            Self::ResourceDirectory(error) => write!(formatter, "{error}"),
            Self::ServiceRegistry(error) => write!(formatter, "service registry error: {error}"),
            Self::Service(windows_service::Error::Winapi(error)) => {
                write!(formatter, "windows service winapi error: {error}")
            }
            Self::Service(error) => write!(formatter, "windows service error: {error}"),
            Self::TimedOut(message) => write!(formatter, "timed out: {message}"),
            Self::UserStartup(error) => write!(formatter, "user startup error: {error}"),
        }
    }
}

impl std::error::Error for InstallerError {}

impl From<std::io::Error> for InstallerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<windows_service::Error> for InstallerError {
    fn from(error: windows_service::Error) -> Self {
        Self::Service(error)
    }
}

impl From<ProviderRegistryError> for InstallerError {
    fn from(error: ProviderRegistryError) -> Self {
        Self::ProviderRegistry(error)
    }
}

impl From<ResourceDirectoryError> for InstallerError {
    fn from(error: ResourceDirectoryError) -> Self {
        Self::ResourceDirectory(error)
    }
}

impl From<ServiceRegistryError> for InstallerError {
    fn from(error: ServiceRegistryError) -> Self {
        Self::ServiceRegistry(error)
    }
}

impl From<UserStartupError> for InstallerError {
    fn from(error: UserStartupError) -> Self {
        Self::UserStartup(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_install_requires_camera_worker_beside_binary() -> Result<(), InstallerError> {
        let root = std::env::temp_dir().join(format!(
            "wfu-worker-payload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root)?;
        let service = root.join("win_service.exe");
        std::fs::write(&service, b"test-service")?;
        assert!(ensure_service_binary_exists(&service).is_err());
        std::fs::write(root.join("camera_worker.exe"), b"test-worker")?;
        assert!(ensure_service_binary_exists(&service).is_ok());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn upgrade_to_another_directory_keeps_previous_credential_store() -> Result<(), InstallerError>
    {
        let store = credential_store_directory_for_install(
            Path::new(r"D:\Apps\WinFaceUnlock\win_service.exe"),
            Some(Path::new(
                r#""C:\Program Files\WinFaceUnlock\win_service.exe" --service"#,
            )),
        )?;
        assert_eq!(
            store,
            PathBuf::from(r"C:\Program Files\WinFaceUnlock\credential-store")
        );
        Ok(())
    }

    #[test]
    fn new_install_uses_absolute_service_directory() -> Result<(), InstallerError> {
        let store = credential_store_directory_for_install(
            Path::new(r"D:\Apps\WinFaceUnlock\win_service.exe"),
            None,
        )?;
        assert_eq!(
            store,
            PathBuf::from(r"D:\Apps\WinFaceUnlock\credential-store")
        );
        assert!(
            credential_store_directory_for_install(Path::new(r"relative\win_service.exe"), None)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn existing_service_command_is_parsed_without_arguments_in_the_path()
    -> Result<(), InstallerError> {
        assert_eq!(
            service_binary_from_command(Path::new(
                r"D:\Apps\WinFaceUnlock\win_service.exe --service"
            ))?,
            PathBuf::from(r"D:\Apps\WinFaceUnlock\win_service.exe")
        );
        for invalid in [
            r"win_service.exe --service",
            r#""C:\Program Files\WinFaceUnlock\win_service.exe"broken"#,
            r#""C:\Program Files\WinFaceUnlock\win_service.exe"#,
        ] {
            assert!(service_binary_from_command(Path::new(invalid)).is_err());
        }
        Ok(())
    }

    #[test]
    fn repeated_failures_keep_restarting_with_bounded_backoff() {
        let actions = service_failure_actions().actions.unwrap_or_default();
        assert_eq!(actions.len(), 3);
        assert!(
            actions
                .iter()
                .all(|action| action.action_type == ServiceActionType::Restart)
        );
        assert_eq!(actions[2].delay, Duration::from_secs(120));
    }

    #[test]
    fn service_install_plan_uses_service_mode_argument() {
        let plan = ServiceInstallPlan::new(PathBuf::from(r"C:\WinFaceUnlock\win_service.exe"));
        let info = plan.to_service_info();

        assert_eq!(info.name, OsString::from(SERVICE_NAME));
        assert_eq!(info.launch_arguments, vec![OsString::from("--service")]);
        assert_eq!(info.account_name, None);
        assert_eq!(info.start_type, ServiceStartType::AutoStart);
    }
}
