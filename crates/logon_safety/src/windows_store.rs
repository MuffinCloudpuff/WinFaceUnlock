#![allow(unsafe_code)]

use crate::{GuardState, LogonBlock, Phase, WindowsLogonResult};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
    },
    System::{
        Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, RegCloseKey,
            RegCreateKeyExW, RegFlushKey, RegQueryValueExW, RegSetValueExW,
        },
        Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    },
};

const REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\LogonSafety";
const VALUE_NAME: &str = "StateV1";
const MUTEX_NAME: &str = r"Global\WinFaceUnlock.LogonSafety.V1";
const RECORD_LEN: usize = 24;

struct StateLock(HANDLE);
impl StateLock {
    fn acquire() -> Result<Self, LogonBlock> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(LogonBlock::StorageUnavailable);
        }
        match unsafe { WaitForSingleObject(handle, 5_000) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
            _ => {
                unsafe {
                    CloseHandle(handle);
                }
                Err(LogonBlock::StorageUnavailable)
            }
        }
    }
}
impl Drop for StateLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

pub(super) fn transaction<T>(
    operation: impl FnOnce(&mut GuardState) -> Result<T, LogonBlock>,
) -> Result<T, LogonBlock> {
    let _lock = StateLock::acquire()?;
    let path = wide(REGISTRY_PATH);
    let mut key = std::ptr::null_mut();
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(LogonBlock::StorageUnavailable);
    }
    let key = Key(key);
    let name = wide(VALUE_NAME);
    let mut bytes = [0u8; RECORD_LEN];
    let mut size = RECORD_LEN as u32;
    let mut kind = 0;
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            bytes.as_mut_ptr(),
            &mut size,
        )
    };
    let mut state = if code == ERROR_FILE_NOT_FOUND {
        GuardState::default()
    } else if code == ERROR_SUCCESS && kind == REG_BINARY && size == RECORD_LEN as u32 {
        decode(bytes)?
    } else {
        return Err(LogonBlock::StorageUnavailable);
    };
    let before = state;
    let result = operation(&mut state)?;
    if state != before {
        let bytes = encode(state);
        if unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_BINARY,
                bytes.as_ptr(),
                RECORD_LEN as u32,
            )
        } != ERROR_SUCCESS
            || unsafe { RegFlushKey(key.0) } != ERROR_SUCCESS
        {
            return Err(LogonBlock::StorageUnavailable);
        }
    }
    Ok(result)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn encode(state: GuardState) -> [u8; RECORD_LEN] {
    let mut bytes = [0u8; RECORD_LEN];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    bytes[4..12].copy_from_slice(&state.generation.to_le_bytes());
    let (phase, result) = match state.phase {
        Phase::Ready => (
            0u32,
            WindowsLogonResult {
                status: 0,
                substatus: 0,
            },
        ),
        Phase::Submitted => (
            1u32,
            WindowsLogonResult {
                status: 0,
                substatus: 0,
            },
        ),
        Phase::Rejected(result) => (2u32, result),
    };
    bytes[12..16].copy_from_slice(&phase.to_le_bytes());
    bytes[16..20].copy_from_slice(&result.status.to_le_bytes());
    bytes[20..24].copy_from_slice(&result.substatus.to_le_bytes());
    bytes
}

fn decode(bytes: [u8; RECORD_LEN]) -> Result<GuardState, LogonBlock> {
    fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, LogonBlock> {
        Ok(u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| LogonBlock::StorageUnavailable)?,
        ))
    }
    if u32_at(&bytes, 0)? != 1 {
        return Err(LogonBlock::StorageUnavailable);
    }
    let generation = u64::from_le_bytes(
        bytes[4..12]
            .try_into()
            .map_err(|_| LogonBlock::StorageUnavailable)?,
    );
    let result = WindowsLogonResult {
        status: u32_at(&bytes, 16)? as i32,
        substatus: u32_at(&bytes, 20)? as i32,
    };
    let phase = match u32_at(&bytes, 12)? {
        0 if result.succeeded() => Phase::Ready,
        1 if result.succeeded() => Phase::Submitted,
        2 if !result.succeeded() => Phase::Rejected(result),
        _ => return Err(LogonBlock::StorageUnavailable),
    };
    Ok(GuardState { generation, phase })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_pending_and_rejection_survive_reload_without_authenticating() {
        for phase in [
            Phase::Ready,
            Phase::Submitted,
            Phase::Rejected(WindowsLogonResult {
                status: 0xC0000234u32 as i32,
                substatus: 0,
            }),
        ] {
            let state = GuardState {
                generation: 42,
                phase,
            };
            assert_eq!(decode(encode(state)), Ok(state));
        }
    }
    #[test]
    fn corrupt_or_unknown_guard_records_fail_closed() {
        assert_eq!(decode([0; RECORD_LEN]), Err(LogonBlock::StorageUnavailable));
        let mut bytes = encode(GuardState::default());
        bytes[12] = 99;
        assert_eq!(decode(bytes), Err(LogonBlock::StorageUnavailable));
    }
}
