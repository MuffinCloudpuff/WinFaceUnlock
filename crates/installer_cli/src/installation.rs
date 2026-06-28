use crate::{
    provider_registry::{ProviderInstallPlan, ProviderRegistry},
    resource_directory::ResourceDirectoryPlan,
    service_manager::{InstallerError, ServiceInstallPlan, ServiceManagerFacade},
    service_registry::{ServiceAuthRegistry, ServiceAuthRegistryConfig},
    user_startup::{UserStartupPlan, UserStartupRegistry},
};

#[derive(Clone, Debug, PartialEq)]
pub struct FullInstallPlan {
    pub service_plan: ServiceInstallPlan,
    pub provider_plan: ProviderInstallPlan,
    pub resource_plan: ResourceDirectoryPlan,
    pub auth_config: Option<ServiceAuthRegistryConfig>,
    pub user_startup_plan: UserStartupPlan,
    pub start_service: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FullUninstallPlan {
    pub resource_plan: ResourceDirectoryPlan,
    pub stop_service_first: bool,
    pub delete_data: bool,
    pub remove_user_startup: bool,
}

pub struct InstallerOrchestrator;

impl InstallerOrchestrator {
    pub fn install(plan: &FullInstallPlan) -> Result<(), InstallerError> {
        log_step("prepare-resource-directories");
        plan.resource_plan.prepare()?;

        log_step("configure-service-auth");
        if let Some(config) = &plan.auth_config {
            ServiceAuthRegistry::configure_local_camera(config)?;
        } else {
            eprintln!("installer_step_skipped: configure-service-auth");
        }

        log_step("install-or-repair-service");
        let manager = ServiceManagerFacade::connect_for_installation()?;
        manager.repair_service(&plan.service_plan)?;

        if plan.start_service {
            log_step("restart-service");
            manager.restart_service()?;
        }

        log_step("install-provider");
        ProviderRegistry::install_provider(&plan.provider_plan)?;

        log_step("install-user-tray-startup");
        UserStartupRegistry::install_tray_startup(&plan.user_startup_plan)?;
        Ok(())
    }

    pub fn repair(plan: &FullInstallPlan) -> Result<(), InstallerError> {
        log_step("repair-resource-directories");
        plan.resource_plan.prepare()?;

        log_step("repair-service");
        let manager = ServiceManagerFacade::connect_for_installation()?;
        manager.repair_service(&plan.service_plan)?;

        if let Some(config) = &plan.auth_config {
            log_step("repair-service-auth-config");
            ServiceAuthRegistry::configure_local_camera(config)?;
        }

        if plan.start_service {
            log_step("restart-service");
            manager.restart_service()?;
        }

        log_step("repair-provider");
        ProviderRegistry::install_provider(&plan.provider_plan)?;

        log_step("repair-user-tray-startup");
        UserStartupRegistry::install_tray_startup(&plan.user_startup_plan)?;
        Ok(())
    }

    pub fn uninstall(plan: &FullUninstallPlan) -> Result<(), InstallerError> {
        if plan.remove_user_startup {
            log_step("uninstall-user-tray-startup");
            UserStartupRegistry::uninstall_tray_startup()?;
        } else {
            eprintln!("installer_step_skipped: uninstall-user-tray-startup");
        }

        log_step("uninstall-provider");
        ProviderRegistry::uninstall_provider()?;

        log_step("uninstall-service");
        let manager = ServiceManagerFacade::connect()?;
        manager.uninstall_service_if_exists(plan.stop_service_first)?;

        if plan.delete_data {
            log_step("delete-resource-data");
            plan.resource_plan.delete_data()?;
        } else {
            eprintln!("installer_step_skipped: delete-resource-data");
        }
        Ok(())
    }

    pub fn emergency_disable_provider() -> Result<(), InstallerError> {
        log_step("emergency-disable-provider");
        ProviderRegistry::emergency_disable_provider()?;
        Ok(())
    }
}

fn log_step(name: &str) {
    eprintln!("installer_step: {name}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::ProviderInstallPlan;
    use std::path::PathBuf;

    #[test]
    fn full_uninstall_plan_can_request_data_deletion() {
        let plan = FullUninstallPlan {
            resource_plan: ResourceDirectoryPlan::from_root_dir(PathBuf::from(r"C:\WinFaceUnlock")),
            stop_service_first: true,
            delete_data: true,
            remove_user_startup: true,
        };

        assert!(plan.stop_service_first);
        assert!(plan.delete_data);
    }

    #[test]
    fn full_install_plan_keeps_provider_registration_after_service_plan() {
        let plan = FullInstallPlan {
            service_plan: ServiceInstallPlan::new(PathBuf::from(
                r"C:\WinFaceUnlock\win_service.exe",
            )),
            provider_plan: ProviderInstallPlan::new(PathBuf::from(
                r"C:\WinFaceUnlock\windows_provider.dll",
            )),
            resource_plan: ResourceDirectoryPlan::from_root_dir(PathBuf::from(r"C:\WinFaceUnlock")),
            auth_config: None,
            user_startup_plan: UserStartupPlan::new(PathBuf::from(
                r"C:\WinFaceUnlock\control_tray.exe",
            )),
            start_service: true,
        };

        assert_eq!(
            plan.service_plan.service_binary_path,
            PathBuf::from(r"C:\WinFaceUnlock\win_service.exe")
        );
        assert_eq!(
            plan.provider_plan.provider_binary_path,
            PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll")
        );
        assert!(plan.start_service);
        assert_eq!(
            plan.user_startup_plan.tray_binary_path,
            PathBuf::from(r"C:\WinFaceUnlock\control_tray.exe")
        );
    }
}
