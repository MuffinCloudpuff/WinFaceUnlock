#![allow(unsafe_code)]

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

use windows_provider::{
    COM_CLSID_REGISTRY_PATH, COM_INPROC_SERVER_REGISTRY_PATH, PROVIDER_CLSID_BRACED,
    PROVIDER_CLSID_REGISTRY_PATH, PROVIDER_ROOT_REGISTRY_PATH, REG_VALUE_AUTO_WAKE_ON_ADVISE,
    REG_VALUE_TILE_VISIBILITY, REG_VALUE_WAKE_AUTH_SOURCE, TILE_VISIBILITY_VISIBLE,
    WAKE_AUTH_SOURCE_LOCAL_CAMERA, WINDOWS_PROVIDER_NAME,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderInstallPlan {
    pub provider_name: &'static str,
    pub provider_clsid: &'static str,
    pub provider_binary_path: PathBuf,
    pub credential_provider_registry_path: &'static str,
    pub com_clsid_registry_path: &'static str,
    pub com_inproc_server_registry_path: &'static str,
    pub project_registry_path: &'static str,
    pub tile_visibility: &'static str,
    pub auto_wake_on_advise: bool,
    pub wake_auth_source: &'static str,
}

impl ProviderInstallPlan {
    pub fn new(provider_binary_path: PathBuf) -> Self {
        Self {
            provider_name: WINDOWS_PROVIDER_NAME,
            provider_clsid: PROVIDER_CLSID_BRACED,
            provider_binary_path,
            credential_provider_registry_path: PROVIDER_CLSID_REGISTRY_PATH,
            com_clsid_registry_path: COM_CLSID_REGISTRY_PATH,
            com_inproc_server_registry_path: COM_INPROC_SERVER_REGISTRY_PATH,
            project_registry_path: PROVIDER_ROOT_REGISTRY_PATH,
            tile_visibility: TILE_VISIBILITY_VISIBLE,
            auto_wake_on_advise: false,
            wake_auth_source: WAKE_AUTH_SOURCE_LOCAL_CAMERA,
        }
    }

    pub fn with_auto_wake_on_advise(mut self, auto_wake_on_advise: bool) -> Self {
        self.auto_wake_on_advise = auto_wake_on_advise;
        self
    }

    pub fn with_wake_auth_source(mut self, wake_auth_source: &'static str) -> Self {
        self.wake_auth_source = wake_auth_source;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRegistryStatus {
    pub credential_provider_registered: bool,
    pub com_server_registered: bool,
    pub project_config_registered: bool,
}

impl ProviderRegistryStatus {
    pub fn is_registered(&self) -> bool {
        self.credential_provider_registered
            && self.com_server_registered
            && self.project_config_registered
    }
}

pub struct ProviderRegistry;

impl ProviderRegistry {
    pub fn install_provider(plan: &ProviderInstallPlan) -> Result<(), ProviderRegistryError> {
        ensure_provider_binary_exists(&plan.provider_binary_path)?;

        registry::set_default_string(plan.com_clsid_registry_path, plan.provider_name)?;
        registry::set_default_string(
            plan.com_inproc_server_registry_path,
            &plan.provider_binary_path.display().to_string(),
        )?;
        registry::set_string_value(
            plan.com_inproc_server_registry_path,
            "ThreadingModel",
            "Apartment",
        )?;

        registry::set_default_string(plan.credential_provider_registry_path, plan.provider_name)?;

        registry::set_string_value(
            plan.project_registry_path,
            "ProviderName",
            plan.provider_name,
        )?;
        registry::set_string_value(
            plan.project_registry_path,
            "ProviderClsid",
            plan.provider_clsid,
        )?;
        registry::set_string_value(
            plan.project_registry_path,
            "ProviderDll",
            &plan.provider_binary_path.display().to_string(),
        )?;
        registry::set_string_value(plan.project_registry_path, "SchemaVersion", "1")?;
        registry::set_string_value(
            plan.project_registry_path,
            REG_VALUE_TILE_VISIBILITY,
            plan.tile_visibility,
        )?;
        registry::set_string_value(
            plan.project_registry_path,
            REG_VALUE_AUTO_WAKE_ON_ADVISE,
            bool_registry_value(plan.auto_wake_on_advise),
        )?;
        registry::set_string_value(
            plan.project_registry_path,
            REG_VALUE_WAKE_AUTH_SOURCE,
            plan.wake_auth_source,
        )?;
        Ok(())
    }

    pub fn uninstall_provider() -> Result<(), ProviderRegistryError> {
        registry::delete_tree(PROVIDER_CLSID_REGISTRY_PATH)?;
        registry::delete_tree(COM_CLSID_REGISTRY_PATH)?;
        registry::delete_tree(PROVIDER_ROOT_REGISTRY_PATH)?;
        Ok(())
    }

    pub fn emergency_disable_provider() -> Result<(), ProviderRegistryError> {
        registry::delete_tree(PROVIDER_CLSID_REGISTRY_PATH)
    }

    pub fn provider_status() -> ProviderRegistryStatus {
        ProviderRegistryStatus {
            credential_provider_registered: registry::key_exists(PROVIDER_CLSID_REGISTRY_PATH),
            com_server_registered: registry::key_exists(COM_INPROC_SERVER_REGISTRY_PATH),
            project_config_registered: registry::key_exists(PROVIDER_ROOT_REGISTRY_PATH),
        }
    }
}

fn ensure_provider_binary_exists(provider_binary_path: &Path) -> Result<(), ProviderRegistryError> {
    if provider_binary_path.is_file() {
        Ok(())
    } else {
        Err(ProviderRegistryError::InvalidProviderBinary(format!(
            "provider dll does not exist: {}",
            provider_binary_path.display()
        )))
    }
}

fn bool_registry_value(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

#[derive(Debug)]
pub enum ProviderRegistryError {
    InvalidProviderBinary(String),
    WindowsRegistry {
        operation: &'static str,
        path: String,
        code: u32,
    },
}

impl fmt::Display for ProviderRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProviderBinary(message) => write!(formatter, "{message}"),
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

impl std::error::Error for ProviderRegistryError {}

pub fn default_provider_binary_path() -> Result<PathBuf, std::io::Error> {
    let installer_path = std::env::current_exe()?;
    Ok(installer_path.with_file_name(provider_binary_file_name()))
}

fn provider_binary_file_name() -> OsString {
    if cfg!(windows) {
        OsString::from("windows_provider.dll")
    } else {
        OsString::from("libwindows_provider.so")
    }
}

fn to_wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
            RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW,
        },
    };

    use super::{ProviderRegistryError, to_wide_null};

    pub fn set_default_string(path: &str, value: &str) -> Result<(), ProviderRegistryError> {
        set_string_value(path, "", value)
    }

    pub fn set_string_value(
        path: &str,
        value_name: &str,
        value: &str,
    ) -> Result<(), ProviderRegistryError> {
        let key = create_key(path)?;
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

    pub fn delete_tree(path: &str) -> Result<(), ProviderRegistryError> {
        let path_wide = to_wide_null(path);
        let status = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, path_wide.as_ptr()) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(registry_error("delete tree", path, status))
        }
    }

    pub fn key_exists(path: &str) -> bool {
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
            unsafe {
                let _ = RegCloseKey(key);
            }
            true
        } else {
            false
        }
    }

    fn create_key(path: &str) -> Result<OwnedRegistryKey, ProviderRegistryError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("create key", path, status));
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

    fn registry_error(operation: &'static str, path: &str, code: u32) -> ProviderRegistryError {
        ProviderRegistryError::WindowsRegistry {
            operation,
            path: path.to_owned(),
            code,
        }
    }
}

#[cfg(not(windows))]
mod registry {
    use super::ProviderRegistryError;

    pub fn set_default_string(path: &str, _value: &str) -> Result<(), ProviderRegistryError> {
        Err(unsupported(path, "set default value"))
    }

    pub fn set_string_value(
        path: &str,
        _value_name: &str,
        _value: &str,
    ) -> Result<(), ProviderRegistryError> {
        Err(unsupported(path, "set value"))
    }

    pub fn delete_tree(path: &str) -> Result<(), ProviderRegistryError> {
        Err(unsupported(path, "delete tree"))
    }

    pub fn key_exists(_path: &str) -> bool {
        false
    }

    fn unsupported(path: &str, operation: &'static str) -> ProviderRegistryError {
        ProviderRegistryError::WindowsRegistry {
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
    fn install_plan_uses_project_owned_provider_identity() {
        let plan =
            ProviderInstallPlan::new(PathBuf::from(r"C:\WinFaceUnlock\windows_provider.dll"));

        assert_eq!(plan.provider_name, WINDOWS_PROVIDER_NAME);
        assert_eq!(plan.provider_clsid, PROVIDER_CLSID_BRACED);
        assert!(
            plan.credential_provider_registry_path
                .contains(PROVIDER_CLSID_BRACED)
        );
        assert!(
            plan.com_inproc_server_registry_path
                .contains(PROVIDER_CLSID_BRACED)
        );
    }

    #[test]
    fn provider_registry_status_requires_all_registration_layers() {
        let partial = ProviderRegistryStatus {
            credential_provider_registered: true,
            com_server_registered: true,
            project_config_registered: false,
        };

        assert!(!partial.is_registered());
    }
}
