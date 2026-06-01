use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    thread::sleep,
    time::{Duration, Instant},
};

use common_protocol::SERVICE_NAME;

use crate::{provider_registry::ProviderRegistryError, service_registry::ServiceRegistryError};
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

    pub fn query_service_status(&self) -> Result<ServiceStatus, InstallerError> {
        let service = self
            .manager
            .open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
        Ok(service.query_status()?)
    }
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
    service.set_delayed_auto_start(true)?;
    service.update_failure_actions(ServiceFailureActions {
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
                action_type: ServiceActionType::None,
                delay: Duration::default(),
            },
        ]),
    })?;
    service.set_failure_actions_on_non_crash_failures(true)?;
    Ok(())
}

fn ensure_service_binary_exists(service_binary_path: &Path) -> Result<(), InstallerError> {
    if service_binary_path.is_file() {
        Ok(())
    } else {
        Err(InstallerError::InvalidArguments(format!(
            "service binary does not exist: {}",
            service_binary_path.display()
        )))
    }
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
    ServiceRegistry(ServiceRegistryError),
    Service(windows_service::Error),
    TimedOut(String),
}

impl fmt::Display for InstallerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(message) => write!(formatter, "invalid arguments: {message}"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::ProviderRegistry(error) => write!(formatter, "provider registry error: {error}"),
            Self::ServiceRegistry(error) => write!(formatter, "service registry error: {error}"),
            Self::Service(windows_service::Error::Winapi(error)) => {
                write!(formatter, "windows service winapi error: {error}")
            }
            Self::Service(error) => write!(formatter, "windows service error: {error}"),
            Self::TimedOut(message) => write!(formatter, "timed out: {message}"),
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

impl From<ServiceRegistryError> for InstallerError {
    fn from(error: ServiceRegistryError) -> Self {
        Self::ServiceRegistry(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
