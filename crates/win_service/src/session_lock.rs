#[derive(Debug, Eq, PartialEq)]
pub enum SessionLockError {
    PlatformUnavailable,
    LockRequestFailed,
    UserSessionTokenUnavailable,
    UserSessionLockProcessLaunchFailed,
}

pub trait SessionLocker {
    fn request_lock_workstation(&self) -> Result<(), SessionLockError>;
}

pub struct WindowsSessionLocker {
    target_session_id: Option<u32>,
}

impl WindowsSessionLocker {
    pub fn current_workstation() -> Self {
        Self {
            target_session_id: None,
        }
    }

    pub fn user_session(session_id: u32) -> Self {
        Self {
            target_session_id: Some(session_id),
        }
    }
}

impl SessionLocker for WindowsSessionLocker {
    fn request_lock_workstation(&self) -> Result<(), SessionLockError> {
        match self.target_session_id {
            Some(session_id) => request_lock_user_session(session_id),
            None => request_lock_workstation(),
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn request_lock_workstation() -> Result<(), SessionLockError> {
    let lock_succeeded = unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
    if lock_succeeded == 0 {
        return Err(SessionLockError::LockRequestFailed);
    }
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub fn request_lock_user_session(session_id: u32) -> Result<(), SessionLockError> {
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
    };

    let user_token = UserSessionToken::query(session_id)?;
    let mut desktop_name = to_wide_null(r"winsta0\default");
    let startup_info = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: desktop_name.as_mut_ptr(),
        ..STARTUPINFOW::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();
    let mut command_line = to_wide_null(r"rundll32.exe user32.dll,LockWorkStation");

    let launch_succeeded = unsafe {
        CreateProcessAsUserW(
            user_token.raw,
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &startup_info,
            &mut process_info,
        )
    };
    if launch_succeeded == 0 {
        return Err(SessionLockError::UserSessionLockProcessLaunchFailed);
    }
    let process = OwnedHandle::new(process_info.hProcess);
    let thread = OwnedHandle::new(process_info.hThread);
    unsafe {
        let _ = WaitForSingleObject(process.raw, 5_000);
    }
    drop(thread);
    drop(process);
    Ok(())
}

#[cfg(windows)]
struct UserSessionToken {
    raw: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl UserSessionToken {
    #[allow(unsafe_code)]
    fn query(session_id: u32) -> Result<Self, SessionLockError> {
        use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;

        let mut token = std::ptr::null_mut();
        let token_succeeded = unsafe { WTSQueryUserToken(session_id, &mut token) };
        if token_succeeded == 0 {
            return Err(SessionLockError::UserSessionTokenUnavailable);
        }
        Ok(Self { raw: token })
    }
}

#[cfg(windows)]
impl Drop for UserSessionToken {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(self.raw);
        }
    }
}

#[cfg(windows)]
struct OwnedHandle {
    raw: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl OwnedHandle {
    fn new(raw: windows_sys::Win32::Foundation::HANDLE) -> Self {
        Self { raw }
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(self.raw);
            }
        }
    }
}

#[cfg(windows)]
fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
pub fn request_lock_workstation() -> Result<(), SessionLockError> {
    Err(SessionLockError::PlatformUnavailable)
}

#[cfg(not(windows))]
pub fn request_lock_user_session(_session_id: u32) -> Result<(), SessionLockError> {
    Err(SessionLockError::PlatformUnavailable)
}
