use std::{ffi::OsString, path::PathBuf};

use crate::service_manager::{
    InstallerError, ServiceInstallPlan, ServiceLifecycleCommand, ServiceManagerFacade,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallerCommand {
    InstallService {
        service_binary_path: PathBuf,
        start_after_install: bool,
        dry_run: bool,
    },
    UninstallService {
        stop_first: bool,
        dry_run: bool,
    },
    StartService,
    StopService,
    ServiceStatus,
    RepairService {
        service_binary_path: PathBuf,
        dry_run: bool,
    },
    Help,
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> Result<(), InstallerError> {
    let args: Vec<String> = args.into_iter().collect();
    let command = parse_command(&args)?;
    run_command(command)
}

fn run_command(command: InstallerCommand) -> Result<(), InstallerError> {
    match command {
        InstallerCommand::InstallService {
            service_binary_path,
            start_after_install,
            dry_run,
        } => {
            let plan = ServiceInstallPlan::new(service_binary_path);
            if dry_run {
                print_install_plan("install-service dry-run", &plan);
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect_for_installation()?;
            manager.install_service(&plan)?;
            println!("installed {}", plan.service_name.to_string_lossy());
            if start_after_install {
                manager.run_lifecycle_command(ServiceLifecycleCommand::Start)?;
                println!("started {}", plan.service_name.to_string_lossy());
            }
        }
        InstallerCommand::UninstallService {
            stop_first,
            dry_run,
        } => {
            if dry_run {
                println!("uninstall-service dry-run: service will be stopped first: {stop_first}");
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect()?;
            manager.uninstall_service(stop_first)?;
            println!("uninstalled WinFaceUnlockService");
        }
        InstallerCommand::StartService => {
            let manager = ServiceManagerFacade::connect()?;
            manager.run_lifecycle_command(ServiceLifecycleCommand::Start)?;
            println!("started WinFaceUnlockService");
        }
        InstallerCommand::StopService => {
            let manager = ServiceManagerFacade::connect()?;
            manager.run_lifecycle_command(ServiceLifecycleCommand::Stop)?;
            println!("stopped WinFaceUnlockService");
        }
        InstallerCommand::ServiceStatus => {
            let manager = ServiceManagerFacade::connect()?;
            let status = manager.query_service_status()?;
            println!("WinFaceUnlockService status: {:?}", status.current_state);
            if let Some(process_id) = status.process_id {
                println!("process_id: {process_id}");
            }
        }
        InstallerCommand::RepairService {
            service_binary_path,
            dry_run,
        } => {
            let plan = ServiceInstallPlan::new(service_binary_path);
            if dry_run {
                print_install_plan("repair-service dry-run", &plan);
                return Ok(());
            }
            let manager = ServiceManagerFacade::connect_for_installation()?;
            manager.repair_service(&plan)?;
            println!("repaired {}", plan.service_name.to_string_lossy());
        }
        InstallerCommand::Help => print_usage(),
    }

    Ok(())
}

fn parse_command(args: &[String]) -> Result<InstallerCommand, InstallerError> {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let service_binary_path = argument_value(args, "--service-binary")
        .map(PathBuf::from)
        .unwrap_or(default_service_binary_path()?);
    let dry_run = has_flag(args, "--dry-run");

    match command {
        "install-service" => Ok(InstallerCommand::InstallService {
            service_binary_path,
            start_after_install: has_flag(args, "--start"),
            dry_run,
        }),
        "uninstall-service" => Ok(InstallerCommand::UninstallService {
            stop_first: !has_flag(args, "--no-stop"),
            dry_run,
        }),
        "start-service" => Ok(InstallerCommand::StartService),
        "stop-service" => Ok(InstallerCommand::StopService),
        "service-status" => Ok(InstallerCommand::ServiceStatus),
        "repair-service" => Ok(InstallerCommand::RepairService {
            service_binary_path,
            dry_run,
        }),
        "help" | "--help" | "-h" => Ok(InstallerCommand::Help),
        other => Err(InstallerError::InvalidArguments(format!(
            "unknown command: {other}"
        ))),
    }
}

fn default_service_binary_path() -> Result<PathBuf, InstallerError> {
    let installer_path = std::env::current_exe().map_err(InstallerError::Io)?;
    Ok(installer_path.with_file_name(service_binary_file_name()))
}

fn service_binary_file_name() -> OsString {
    if cfg!(windows) {
        OsString::from("win_service.exe")
    } else {
        OsString::from("win_service")
    }
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn argument_value<'args>(args: &'args [String], name: &str) -> Option<&'args str> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].as_str())
}

fn print_install_plan(label: &str, plan: &ServiceInstallPlan) {
    println!("{label}");
    println!("service_name: {}", plan.service_name.to_string_lossy());
    println!("display_name: {}", plan.display_name.to_string_lossy());
    println!("binary: {}", plan.service_binary_path.display());
    println!("launch_argument: --service");
    println!("account: LocalSystem");
}

fn print_usage() {
    println!("WinFaceUnlock installer");
    println!("Usage:");
    println!("  installer_cli install-service [--service-binary <path>] [--start] [--dry-run]");
    println!("  installer_cli uninstall-service [--no-stop] [--dry-run]");
    println!("  installer_cli start-service");
    println!("  installer_cli stop-service");
    println!("  installer_cli service-status");
    println!("  installer_cli repair-service [--service-binary <path>] [--dry-run]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_install_service_with_explicit_binary_and_start() -> Result<(), InstallerError> {
        let args = vec![
            "installer_cli".to_owned(),
            "install-service".to_owned(),
            "--service-binary".to_owned(),
            r"C:\WinFaceUnlock\win_service.exe".to_owned(),
            "--start".to_owned(),
        ];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::InstallService {
                service_binary_path: PathBuf::from(r"C:\WinFaceUnlock\win_service.exe"),
                start_after_install: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn parse_uninstall_service_defaults_to_stop_first() -> Result<(), InstallerError> {
        let args = vec!["installer_cli".to_owned(), "uninstall-service".to_owned()];

        assert_eq!(
            parse_command(&args)?,
            InstallerCommand::UninstallService {
                stop_first: true,
                dry_run: false,
            }
        );
        Ok(())
    }

    #[test]
    fn argument_value_reads_service_binary_path() {
        let args = vec![
            "installer_cli".to_owned(),
            "repair-service".to_owned(),
            "--service-binary".to_owned(),
            "service.exe".to_owned(),
        ];

        assert_eq!(
            argument_value(&args, "--service-binary"),
            Some("service.exe")
        );
    }
}
