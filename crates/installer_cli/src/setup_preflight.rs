use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use setup_api::{PreflightPayload, RequiredPayloadFile, SetupPreflightCheck, SetupStepStatus};

use crate::resource_directory::ResourceDirectoryPlan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightOutcome {
    pub checks: Vec<SetupPreflightCheck>,
    pub missing_payload_files: Vec<MissingPayloadFile>,
}

impl PreflightOutcome {
    pub fn all_required_checks_passed(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|check| check.status == SetupStepStatus::Failed)
    }

    pub fn requires_elevation(&self) -> bool {
        self.checks.iter().any(|check| {
            check.check_id == "process_elevated" && check.status == SetupStepStatus::Failed
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingPayloadFile {
    pub file_id: String,
    pub path: PathBuf,
}

pub fn run_preflight(payload: &PreflightPayload) -> PreflightOutcome {
    let mut checks = vec![
        check_install_dir_shape(&payload.install_dir),
        check_install_dir_writable(&payload.install_dir),
        check_process_elevation(payload.require_elevation),
        check_recovery_entry_point(),
        check_program_data_root(),
    ];

    let (payload_check, missing_payload_files) =
        check_required_payload_files(&payload.required_payload_files);
    checks.push(payload_check);

    PreflightOutcome {
        checks,
        missing_payload_files,
    }
}

fn check_install_dir_shape(install_dir: &Path) -> SetupPreflightCheck {
    if install_dir.as_os_str().is_empty() {
        return SetupPreflightCheck::failed("install_dir_valid", "Install directory is empty.");
    }
    if !install_dir.is_absolute() {
        return SetupPreflightCheck::failed(
            "install_dir_valid",
            "Install directory must be an absolute path.",
        );
    }
    if install_dir.parent().is_none() || install_dir.file_name().is_none() {
        return SetupPreflightCheck::failed(
            "install_dir_valid",
            "Install directory must not be a drive root.",
        );
    }

    SetupPreflightCheck::succeeded("install_dir_valid", "Install directory path is valid.")
}

fn check_install_dir_writable(install_dir: &Path) -> SetupPreflightCheck {
    let probe_dir = if install_dir.exists() {
        if !install_dir.is_dir() {
            return SetupPreflightCheck::failed(
                "install_dir_writable",
                "Install path already exists and is not a directory.",
            );
        }
        install_dir.to_path_buf()
    } else {
        match install_dir.parent() {
            Some(parent) if parent.is_dir() => parent.to_path_buf(),
            _ => {
                return SetupPreflightCheck::failed(
                    "install_dir_writable",
                    "Install directory parent does not exist.",
                );
            }
        }
    };

    match create_and_remove_probe_file(&probe_dir) {
        Ok(()) => SetupPreflightCheck::succeeded(
            "install_dir_writable",
            "Install directory location is writable.",
        ),
        Err(error) => SetupPreflightCheck::failed(
            "install_dir_writable",
            format!("Install directory location is not writable: {error}"),
        ),
    }
}

fn create_and_remove_probe_file(probe_dir: &Path) -> Result<(), std::io::Error> {
    let probe_path = probe_dir.join(format!(
        ".winfaceunlock-preflight-{}",
        unique_probe_suffix()
    ));
    let create_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path);
    match create_result {
        Ok(_) => {
            fs::remove_file(&probe_path)?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn unique_probe_suffix() -> String {
    let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(_) => 0,
    };
    format!("{}-{timestamp}", std::process::id())
}

fn check_process_elevation(require_elevation: bool) -> SetupPreflightCheck {
    if !require_elevation {
        return SetupPreflightCheck::skipped(
            "process_elevated",
            "Elevation was not required for this preflight request.",
        );
    }

    if process_has_admin_token() {
        SetupPreflightCheck::succeeded("process_elevated", "Process is elevated.")
    } else {
        SetupPreflightCheck::failed(
            "process_elevated",
            "Setup backend must be elevated before privileged install operations.",
        )
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn process_has_admin_token() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(not(windows))]
fn process_has_admin_token() -> bool {
    false
}

fn check_recovery_entry_point() -> SetupPreflightCheck {
    match std::env::current_exe() {
        Ok(path) if path.is_file() => SetupPreflightCheck::succeeded(
            "recovery_entry_point_available",
            "Setup backend executable is available for recovery commands.",
        ),
        Ok(path) => SetupPreflightCheck::failed(
            "recovery_entry_point_available",
            format!("Setup backend executable is not a file: {}", path.display()),
        ),
        Err(error) => SetupPreflightCheck::failed(
            "recovery_entry_point_available",
            format!("Setup backend executable could not be resolved: {error}"),
        ),
    }
}

fn check_program_data_root() -> SetupPreflightCheck {
    let plan = ResourceDirectoryPlan::from_environment_or_default();
    if plan.root_dir.file_name().and_then(|name| name.to_str()) == Some("WinFaceUnlock") {
        SetupPreflightCheck::succeeded(
            "program_data_root_valid",
            "ProgramData root is scoped to WinFaceUnlock.",
        )
    } else {
        SetupPreflightCheck::failed(
            "program_data_root_valid",
            format!(
                "ProgramData root is not scoped to WinFaceUnlock: {}",
                plan.root_dir.display()
            ),
        )
    }
}

fn check_required_payload_files(
    required_files: &[RequiredPayloadFile],
) -> (SetupPreflightCheck, Vec<MissingPayloadFile>) {
    if required_files.is_empty() {
        return (
            SetupPreflightCheck::skipped(
                "payload_complete",
                "No required payload files were supplied.",
            ),
            Vec::new(),
        );
    }

    let missing_payload_files = required_files
        .iter()
        .filter(|file| !file.path.is_file())
        .map(|file| MissingPayloadFile {
            file_id: file.file_id.clone(),
            path: file.path.clone(),
        })
        .collect::<Vec<_>>();

    if missing_payload_files.is_empty() {
        (
            SetupPreflightCheck::succeeded("payload_complete", "Required payload files exist."),
            missing_payload_files,
        )
    } else {
        (
            SetupPreflightCheck::failed(
                "payload_complete",
                format!(
                    "{} required payload file(s) are missing.",
                    missing_payload_files.len()
                ),
            ),
            missing_payload_files,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_dir_shape_rejects_relative_path() {
        let check = check_install_dir_shape(Path::new("WinFaceUnlock"));

        assert_eq!(check.status, SetupStepStatus::Failed);
        assert_eq!(check.check_id, "install_dir_valid");
    }

    #[test]
    fn payload_check_reports_missing_file_without_plaintext_secrets() {
        let missing_path = std::env::temp_dir().join(format!(
            "winfaceunlock-missing-payload-{}",
            unique_probe_suffix()
        ));
        let required_files = vec![RequiredPayloadFile {
            file_id: "win_service".to_owned(),
            path: missing_path.clone(),
        }];

        let (check, missing_payload_files) = check_required_payload_files(&required_files);

        assert_eq!(check.status, SetupStepStatus::Failed);
        assert_eq!(
            missing_payload_files,
            vec![MissingPayloadFile {
                file_id: "win_service".to_owned(),
                path: missing_path,
            }]
        );
    }

    #[test]
    fn writable_check_accepts_existing_temp_directory() {
        let temp_dir = std::env::temp_dir();

        let check = check_install_dir_writable(&temp_dir);

        assert_eq!(check.status, SetupStepStatus::Succeeded);
    }
}
