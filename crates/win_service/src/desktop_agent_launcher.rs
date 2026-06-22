use std::{path::PathBuf, time::Duration};

use crate::desktop_input_state::desktop_input_snapshot_path;

#[derive(Debug, Eq, PartialEq)]
pub enum DesktopInputAgentLaunchError {
    PlatformUnavailable,
    CurrentExePathUnavailable,
    AgentExeMissing,
    UserSessionTokenUnavailable,
    LaunchFailed,
}

pub fn ensure_desktop_input_presence_agent(
    session_id: u32,
    sample_interval: Duration,
) -> Result<(), DesktopInputAgentLaunchError> {
    let agent_exe = desktop_input_agent_path()?;
    launch_desktop_input_presence_agent(session_id, agent_exe, sample_interval)
}

fn desktop_input_agent_path() -> Result<PathBuf, DesktopInputAgentLaunchError> {
    let current_exe = std::env::current_exe()
        .map_err(|_| DesktopInputAgentLaunchError::CurrentExePathUnavailable)?;
    let current_dir = current_exe
        .parent()
        .ok_or(DesktopInputAgentLaunchError::CurrentExePathUnavailable)?;
    let agent_exe = current_dir.join("desktop_input_agent.exe");
    if !agent_exe.is_file() {
        return Err(DesktopInputAgentLaunchError::AgentExeMissing);
    }
    Ok(agent_exe)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn launch_desktop_input_presence_agent(
    session_id: u32,
    agent_exe: PathBuf,
    sample_interval: Duration,
) -> Result<(), DesktopInputAgentLaunchError> {
    use windows_sys::Win32::System::{
        RemoteDesktop::WTSQueryUserToken,
        Threading::{CREATE_NO_WINDOW, CreateProcessAsUserW, PROCESS_INFORMATION, STARTUPINFOW},
    };

    let mut token = std::ptr::null_mut();
    let token_succeeded = unsafe { WTSQueryUserToken(session_id, &mut token) };
    if token_succeeded == 0 {
        return Err(DesktopInputAgentLaunchError::UserSessionTokenUnavailable);
    }
    let _token = OwnedHandle::new(token);

    let snapshot_path = desktop_input_snapshot_path();
    let sample_interval_ms = sample_interval.as_millis().min(u128::from(u64::MAX)) as u64;
    let command_line = format!(
        "\"{}\" --session-id {} --snapshot-path \"{}\" --sample-interval-ms {}",
        agent_exe.display(),
        session_id,
        snapshot_path.display(),
        sample_interval_ms
    );
    let mut command_line = to_wide_null(&command_line);
    let mut desktop_name = to_wide_null(r"winsta0\default");
    let startup_info = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        lpDesktop: desktop_name.as_mut_ptr(),
        ..STARTUPINFOW::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();
    let launch_succeeded = unsafe {
        CreateProcessAsUserW(
            token,
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &startup_info,
            &mut process_info,
        )
    };
    if launch_succeeded == 0 {
        return Err(DesktopInputAgentLaunchError::LaunchFailed);
    }
    let _process = OwnedHandle::new(process_info.hProcess);
    let _thread = OwnedHandle::new(process_info.hThread);
    Ok(())
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
fn launch_desktop_input_presence_agent(
    _session_id: u32,
    _agent_exe: PathBuf,
    _sample_interval: Duration,
) -> Result<(), DesktopInputAgentLaunchError> {
    Err(DesktopInputAgentLaunchError::PlatformUnavailable)
}
