use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

const APP_DATA_DIR_NAME: &str = "WinFaceUnlock";
const PRESENCE_AUDIT_DIR_NAME: &str = "presence-audit";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDirectoryPlan {
    pub root_dir: PathBuf,
    pub presence_audit_dir: PathBuf,
}

impl ResourceDirectoryPlan {
    pub fn from_environment_or_default() -> Self {
        let root_dir = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(APP_DATA_DIR_NAME);
        Self::from_root_dir(root_dir)
    }

    pub fn from_root_dir(root_dir: PathBuf) -> Self {
        Self {
            presence_audit_dir: root_dir.join(PRESENCE_AUDIT_DIR_NAME),
            root_dir,
        }
    }

    pub fn prepare(&self) -> Result<(), ResourceDirectoryError> {
        fs::create_dir_all(&self.root_dir)?;
        fs::create_dir_all(&self.presence_audit_dir)?;
        apply_restricted_acl(&self.root_dir)
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
    if file_name == APP_DATA_DIR_NAME {
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
        let plan =
            ResourceDirectoryPlan::from_root_dir(PathBuf::from(r"C:\ProgramData\WinFaceUnlock"));

        assert_eq!(
            plan.root_dir,
            PathBuf::from(r"C:\ProgramData\WinFaceUnlock")
        );
        assert_eq!(
            plan.presence_audit_dir,
            PathBuf::from(r"C:\ProgramData\WinFaceUnlock\presence-audit")
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
