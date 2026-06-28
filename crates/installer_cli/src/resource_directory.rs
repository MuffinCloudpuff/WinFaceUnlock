use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const PRESENCE_AUDIT_DIR_NAME: &str = "presence-audit";
const RUNTIME_DIR_NAME: &str = "runtime";
const LOGS_DIR_NAME: &str = "logs";
const CREDENTIAL_STORE_DIR_NAME: &str = "credential-store";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDirectoryPlan {
    pub root_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub credential_store_dir: PathBuf,
    pub presence_audit_dir: PathBuf,
}

impl ResourceDirectoryPlan {
    pub fn from_environment_or_default() -> Self {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .map(Self::from_root_dir)
            .unwrap_or_else(|| Self::from_root_dir(std::env::temp_dir().join("WinFaceUnlock")))
    }

    pub fn from_root_dir(root_dir: PathBuf) -> Self {
        Self {
            runtime_dir: root_dir.join(RUNTIME_DIR_NAME),
            logs_dir: root_dir.join(LOGS_DIR_NAME),
            credential_store_dir: root_dir.join(CREDENTIAL_STORE_DIR_NAME),
            presence_audit_dir: root_dir.join(PRESENCE_AUDIT_DIR_NAME),
            root_dir,
        }
    }

    pub fn prepare(&self) -> Result<(), ResourceDirectoryError> {
        fs::create_dir_all(&self.root_dir)?;
        fs::create_dir_all(&self.runtime_dir)?;
        fs::create_dir_all(&self.logs_dir)?;
        fs::create_dir_all(&self.credential_store_dir)?;
        fs::create_dir_all(&self.presence_audit_dir)?;
        apply_program_files_acl(&self.root_dir)?;
        apply_user_writable_acl(&self.runtime_dir)?;
        apply_user_writable_acl(&self.logs_dir)?;
        apply_restricted_acl(&self.credential_store_dir)?;
        apply_restricted_acl(&self.presence_audit_dir)
    }

    pub fn delete_data(&self) -> Result<(), ResourceDirectoryError> {
        ensure_project_data_root(&self.root_dir)?;
        if self.root_dir.exists() {
            fs::remove_dir_all(&self.root_dir)?;
        }
        Ok(())
    }
}

fn ensure_project_data_root(path: &Path) -> Result<(), ResourceDirectoryError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if file_name.eq_ignore_ascii_case("WinFaceUnlock") {
        Ok(())
    } else {
        Err(ResourceDirectoryError::InvalidDataRoot(path.to_path_buf()))
    }
}

#[cfg(windows)]
fn apply_restricted_acl(path: &Path) -> Result<(), ResourceDirectoryError> {
    let status = Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F")
        .arg("*S-1-5-32-544:(OI)(CI)F")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(ResourceDirectoryError::AclCommandFailed(
            status.code().unwrap_or_default(),
        ))
    }
}

#[cfg(windows)]
fn apply_program_files_acl(path: &Path) -> Result<(), ResourceDirectoryError> {
    let status = Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F")
        .arg("*S-1-5-32-544:(OI)(CI)F")
        .arg("*S-1-5-32-545:(OI)(CI)RX") // Users: Read & Execute
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(ResourceDirectoryError::AclCommandFailed(
            status.code().unwrap_or_default(),
        ))
    }
}

#[cfg(windows)]
fn apply_user_writable_acl(path: &Path) -> Result<(), ResourceDirectoryError> {
    let status = Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg("*S-1-5-18:(OI)(CI)F")
        .arg("*S-1-5-32-544:(OI)(CI)F")
        .arg("*S-1-5-11:(OI)(CI)M")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(ResourceDirectoryError::AclCommandFailed(
            status.code().unwrap_or_default(),
        ))
    }
}

#[cfg(not(windows))]
fn apply_restricted_acl(_path: &Path) -> Result<(), ResourceDirectoryError> {
    Ok(())
}

#[cfg(not(windows))]
fn apply_program_files_acl(_path: &Path) -> Result<(), ResourceDirectoryError> {
    Ok(())
}

#[cfg(not(windows))]
fn apply_user_writable_acl(_path: &Path) -> Result<(), ResourceDirectoryError> {
    Ok(())
}

#[derive(Debug)]
pub enum ResourceDirectoryError {
    Io(std::io::Error),
    AclCommandFailed(i32),
    InvalidDataRoot(PathBuf),
}

impl fmt::Display for ResourceDirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "resource directory io error: {error}"),
            Self::AclCommandFailed(code) => {
                write!(
                    formatter,
                    "resource directory acl command failed: exit {code}"
                )
            }
            Self::InvalidDataRoot(path) => {
                write!(
                    formatter,
                    "invalid WinFaceUnlock data root: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ResourceDirectoryError {}

impl From<std::io::Error> for ResourceDirectoryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_directory_plan_uses_project_owned_program_data_root() {
        let plan = ResourceDirectoryPlan::from_root_dir(PathBuf::from(r"D:\Apps\WinFaceUnlock"));

        assert_eq!(plan.root_dir, PathBuf::from(r"D:\Apps\WinFaceUnlock"));
        assert_eq!(
            plan.runtime_dir,
            PathBuf::from(r"D:\Apps\WinFaceUnlock\runtime")
        );
        assert_eq!(plan.logs_dir, PathBuf::from(r"D:\Apps\WinFaceUnlock\logs"));
        assert_eq!(
            plan.credential_store_dir,
            PathBuf::from(r"D:\Apps\WinFaceUnlock\credential-store")
        );
        assert_eq!(
            plan.presence_audit_dir,
            PathBuf::from(r"D:\Apps\WinFaceUnlock\presence-audit")
        );
    }

    #[test]
    fn delete_data_rejects_non_project_root() {
        let plan = ResourceDirectoryPlan::from_root_dir(PathBuf::from(r"C:\ProgramData"));

        assert!(matches!(
            plan.delete_data(),
            Err(ResourceDirectoryError::InvalidDataRoot(_))
        ));
    }
}
