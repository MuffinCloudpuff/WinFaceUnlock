#![allow(unsafe_code)]

use std::{fmt, path::PathBuf};

const STARTUP_REGISTRY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_VALUE_NAME: &str = "WinFaceUnlockTray";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserStartupPlan {
    pub tray_binary_path: PathBuf,
}

impl UserStartupPlan {
    pub fn new(tray_binary_path: PathBuf) -> Self {
        Self { tray_binary_path }
    }

    pub fn startup_command(&self) -> String {
        quote_command_path(&self.tray_binary_path)
    }
}

pub struct UserStartupRegistry;

impl UserStartupRegistry {
    pub fn install_tray_startup(plan: &UserStartupPlan) -> Result<(), UserStartupError> {
        if !plan.tray_binary_path.is_file() {
            return Err(UserStartupError::MissingTrayBinary(
                plan.tray_binary_path.clone(),
            ));
        }
        registry::set_local_machine_string_value(
            STARTUP_REGISTRY_PATH,
            STARTUP_VALUE_NAME,
            &plan.startup_command(),
        )
    }

    pub fn uninstall_tray_startup() -> Result<(), UserStartupError> {
        registry::delete_local_machine_value(STARTUP_REGISTRY_PATH, STARTUP_VALUE_NAME)
    }
}

fn quote_command_path(path: &std::path::Path) -> String {
    format!("\"{}\"", path.display())
}

#[derive(Debug)]
pub enum UserStartupError {
    MissingTrayBinary(PathBuf),
    WindowsRegistry {
        operation: &'static str,
        path: String,
        code: u32,
    },
}

impl fmt::Display for UserStartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTrayBinary(path) => {
                write!(formatter, "tray binary does not exist: {}", path.display())
            }
            Self::WindowsRegistry {
                operation,
                path,
                code,
            } => write!(
                formatter,
                "windows registry {operation} failed for {path}: error {code}"
            ),
        }
    }
}

impl std::error::Error for UserStartupError {}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegOpenKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    };

    use super::UserStartupError;

    pub fn set_local_machine_string_value(
        path: &str,
        value_name: &str,
        value: &str,
    ) -> Result<(), UserStartupError> {
        let key = open_key(path)?;
        let name = to_wide_null(value_name);
        let mut data = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let status = unsafe {
            RegSetValueExW(
                key.raw,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_mut_ptr().cast::<u8>(),
                (data.len() * size_of::<u16>()) as u32,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("set value", path, status));
        }
        Ok(())
    }

    pub fn delete_local_machine_value(path: &str, value_name: &str) -> Result<(), UserStartupError> {
        let key = open_key(path)?;
        let name = to_wide_null(value_name);
        let status = unsafe { RegDeleteValueW(key.raw, name.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        Err(registry_error("delete value", path, status))
    }

    fn open_key(path: &str) -> Result<OwnedRegistryKey, UserStartupError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("open key", path, status));
        }
        Ok(OwnedRegistryKey { raw: key })
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

    fn registry_error(operation: &'static str, path: &str, code: WIN32_ERROR) -> UserStartupError {
        UserStartupError::WindowsRegistry {
            operation,
            path: path.to_owned(),
            code,
        }
    }

    fn to_wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod registry {
    use super::UserStartupError;

    pub fn set_local_machine_string_value(
        path: &str,
        _value_name: &str,
        _value: &str,
    ) -> Result<(), UserStartupError> {
        Err(unsupported(path, "set value"))
    }

    pub fn delete_local_machine_value(
        _path: &str,
        _value_name: &str,
    ) -> Result<(), UserStartupError> {
        Ok(())
    }

    fn unsupported(path: &str, operation: &'static str) -> UserStartupError {
        UserStartupError::WindowsRegistry {
            operation,
            path: path.to_owned(),
            code: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_command_quotes_installed_tray_path() {
        let plan = UserStartupPlan::new(PathBuf::from(r"D:\Apps\WinFaceUnlock\control_tray.exe"));

        assert_eq!(
            plan.startup_command(),
            r#""D:\Apps\WinFaceUnlock\control_tray.exe""#
        );
    }
}
