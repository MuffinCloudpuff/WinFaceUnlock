#![allow(unsafe_code)]

#[cfg(windows)]
pub fn active_user_session_id() -> Option<u32> {
    active_user_session_id_impl()
}

#[cfg(not(windows))]
pub fn active_user_session_id() -> Option<u32> {
    None
}

#[cfg(windows)]
fn active_user_session_id_impl() -> Option<u32> {
    use std::{ptr, slice};

    use windows_sys::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTSFreeMemory, WTSGetActiveConsoleSessionId,
        WTSQuerySessionInformationW, WTSUserName,
    };

    let session_id = unsafe { WTSGetActiveConsoleSessionId() };
    if session_id == u32::MAX {
        return None;
    }

    let mut buffer = ptr::null_mut();
    let mut byte_len = 0_u32;
    let query_succeeded = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session_id,
            WTSUserName,
            &mut buffer,
            &mut byte_len,
        )
    };
    if query_succeeded == 0 || buffer.is_null() || byte_len < 2 {
        if !buffer.is_null() {
            unsafe { WTSFreeMemory(buffer.cast()) };
        }
        return None;
    }

    let char_len = byte_len as usize / size_of::<u16>();
    let username = unsafe { slice::from_raw_parts(buffer, char_len) };
    let has_username = username.iter().any(|value| *value != 0);
    unsafe { WTSFreeMemory(buffer.cast()) };

    has_username.then_some(session_id)
}
