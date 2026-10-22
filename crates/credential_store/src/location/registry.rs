#![cfg_attr(windows, allow(unsafe_code))]

use crate::CredentialStoreError;
use std::path::{Path, PathBuf};

// Kept outside Service so resetting camera/auth settings preserves credentials.
const STORAGE_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\Storage";
const STORE_DIRECTORY_VALUE: &str = "CredentialStoreDir";
const INITIALIZED_VALUE: &str = "StoreInitialized";

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_MULTI_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteValueW, RegFlushKey, RegGetValueW, RegSetValueExW,
    },
};

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
pub(super) fn load() -> Result<Option<PathBuf>, CredentialStoreError> {
    read_string(STORAGE_REGISTRY_PATH, STORE_DIRECTORY_VALUE, RRF_RT_REG_SZ)
}

#[cfg(windows)]
fn read_wide(path: &str, name: &str, flags: u32) -> Result<Option<Vec<u16>>, CredentialStoreError> {
    use std::ptr;
    let path = wide(path);
    let name = wide(name);
    let mut bytes = 0_u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            name.as_ptr(),
            flags,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS || !(2..=65_536).contains(&bytes) || !bytes.is_multiple_of(2) {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    let mut value = vec![0_u16; bytes as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            name.as_ptr(),
            flags,
            ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    value.truncate(bytes as usize / 2);
    Ok(Some(value))
}

#[cfg(windows)]
fn read_string(
    path: &str,
    name: &str,
    flags: u32,
) -> Result<Option<PathBuf>, CredentialStoreError> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let Some(mut value) = read_wide(path, name, flags)? else {
        return Ok(None);
    };
    if value.pop() != Some(0) || value.is_empty() || value.contains(&0) {
        return Err(CredentialStoreError::RecordValidationFailed);
    }
    Ok(Some(PathBuf::from(OsString::from_wide(&value))))
}

#[cfg(windows)]
pub(super) fn initialized() -> Result<bool, CredentialStoreError> {
    match read_string(STORAGE_REGISTRY_PATH, INITIALIZED_VALUE, RRF_RT_REG_SZ)?.as_deref() {
        None => Ok(false),
        Some(value) if value == Path::new("1") => Ok(true),
        Some(value) if value == Path::new("0") => Ok(false),
        _ => Err(CredentialStoreError::RecordValidationFailed),
    }
}

#[cfg(windows)]
pub(super) fn legacy_directory() -> Result<Option<PathBuf>, CredentialStoreError> {
    let service_key = format!(
        r"SYSTEM\CurrentControlSet\Services\{}",
        common_protocol::SERVICE_NAME
    );
    if let Some(environment) = read_wide(&service_key, "Environment", RRF_RT_REG_MULTI_SZ)? {
        for entry in environment
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
        {
            let entry = String::from_utf16(entry)
                .map_err(|_| CredentialStoreError::RecordValidationFailed)?;
            if let Some((name, value)) = entry.split_once('=')
                && name.eq_ignore_ascii_case("WINFACEUNLOCK_STORE_DIR")
            {
                return Ok(Some(PathBuf::from(value)));
            }
        }
    }
    if let Some(path) = read_string(
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        "WINFACEUNLOCK_STORE_DIR",
        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
    )? {
        return Ok(Some(path));
    }
    Ok(std::env::var_os("WINFACEUNLOCK_STORE_DIR").map(PathBuf::from))
}

#[cfg(windows)]
struct StorageKey(HKEY);

#[cfg(windows)]
impl Drop for StorageKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

#[cfg(windows)]
fn key() -> Result<StorageKey, CredentialStoreError> {
    use std::ptr;
    let mut key = ptr::null_mut();
    let path = wide(STORAGE_REGISTRY_PATH);
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(StorageKey(key))
    } else {
        Err(CredentialStoreError::StoreUnavailable)
    }
}

#[cfg(windows)]
pub(super) fn save(path: &Path, initialized: bool) -> Result<(), CredentialStoreError> {
    use std::os::windows::ffi::OsStrExt;
    let key = key()?;
    set_initialized(&key, initialized)?;
    let name = wide(STORE_DIRECTORY_VALUE);
    let value: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    if status != ERROR_SUCCESS || unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
fn set_initialized(key: &StorageKey, initialized: bool) -> Result<(), CredentialStoreError> {
    let name = wide(INITIALIZED_VALUE);
    let value = wide(if initialized { "1" } else { "0" });
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn mark_initialized() -> Result<(), CredentialStoreError> {
    if initialized()? {
        return Ok(());
    }
    let key = key()?;
    set_initialized(&key, true)?;
    if unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn clear() -> Result<(), CredentialStoreError> {
    let key = key()?;
    for name in [STORE_DIRECTORY_VALUE, INITIALIZED_VALUE] {
        let name = wide(name);
        let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            return Err(CredentialStoreError::StoreUnavailable);
        }
    }
    if unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn load() -> Result<Option<PathBuf>, CredentialStoreError> {
    Ok(None)
}
#[cfg(not(windows))]
pub(super) fn save(_: &Path, _: bool) -> Result<(), CredentialStoreError> {
    Err(CredentialStoreError::StoreUnavailable)
}

#[cfg(not(windows))]
pub(super) fn initialized() -> Result<bool, CredentialStoreError> {
    Ok(false)
}
#[cfg(not(windows))]
pub(super) fn mark_initialized() -> Result<(), CredentialStoreError> {
    Err(CredentialStoreError::StoreUnavailable)
}
#[cfg(not(windows))]
pub(super) fn legacy_directory() -> Result<Option<PathBuf>, CredentialStoreError> {
    Ok(std::env::var_os("WINFACEUNLOCK_STORE_DIR").map(PathBuf::from))
}
#[cfg(not(windows))]
pub(super) fn clear() -> Result<(), CredentialStoreError> {
    Ok(())
}
