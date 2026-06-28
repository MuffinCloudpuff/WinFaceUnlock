use std::{path::PathBuf, thread, time::Duration};

use desktop_input::{
    DesktopInputSnapshot, SNAPSHOT_SCHEMA_VERSION, SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO,
    default_snapshot_path, unix_time_ms_now, write_snapshot_atomic,
};

const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 5_000;

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentConfig {
    session_id: u32,
    snapshot_path: PathBuf,
    sample_interval_ms: u64,
    once: bool,
}

fn main() {
    let config = match AgentConfig::from_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let _single_instance_guard = match SingleInstanceGuard::acquire(config.session_id) {
        SingleInstanceStatus::Acquired(guard) => Some(guard),
        SingleInstanceStatus::AlreadyRunning => {
            eprintln!(
                "DesktopInputPresenceAgent.AlreadyRunning session_id={}",
                config.session_id
            );
            return;
        }
        SingleInstanceStatus::Unavailable => None,
    };

    loop {
        match capture_desktop_input_snapshot(config.session_id) {
            Some(snapshot) => {
                if let Err(error) = write_snapshot_atomic(&config.snapshot_path, &snapshot) {
                    eprintln!("DesktopInputPresenceAgent.SnapshotWriteFailed error={error:?}");
                    std::process::exit(1);
                }
                eprintln!(
                    "DesktopInputPresenceAgent.SnapshotWritten session_id={} human_input_quiet_duration_ms={} sampled_at_unix_ms={}",
                    snapshot.session_id,
                    snapshot.human_input_quiet_duration_ms,
                    snapshot.sampled_at_unix_ms
                );
            }
            None => {
                eprintln!("DesktopInputPresenceAgent.CaptureFailed reason=get-last-input-info");
                std::process::exit(1);
            }
        }

        if config.once {
            break;
        }
        thread::sleep(Duration::from_millis(config.sample_interval_ms.max(1)));
    }
}

enum SingleInstanceStatus {
    Acquired(SingleInstanceGuard),
    AlreadyRunning,
    Unavailable,
}

struct SingleInstanceGuard {
    #[allow(dead_code)]
    raw: PlatformMutexHandle,
}

impl SingleInstanceGuard {
    fn acquire(session_id: u32) -> SingleInstanceStatus {
        platform_acquire_single_instance(session_id)
    }
}

impl AgentConfig {
    fn from_args<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut session_id = None;
        let mut snapshot_path = default_snapshot_path();
        let mut sample_interval_ms = DEFAULT_SAMPLE_INTERVAL_MS;
        let mut once = false;
        let mut iter = args.into_iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--session-id" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--session-id requires a value".to_owned())?;
                    session_id = Some(
                        value
                            .parse::<u32>()
                            .map_err(|_| "--session-id must be a u32".to_owned())?,
                    );
                }
                "--snapshot-path" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--snapshot-path requires a value".to_owned())?;
                    snapshot_path = PathBuf::from(value);
                }
                "--sample-interval-ms" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| "--sample-interval-ms requires a value".to_owned())?;
                    sample_interval_ms = value
                        .parse::<u64>()
                        .map_err(|_| "--sample-interval-ms must be a u64".to_owned())?;
                }
                "--once" => once = true,
                "--help" | "-h" => return Err(Self::usage()),
                other => return Err(format!("unknown argument: {other}\n{}", Self::usage())),
            }
        }

        Ok(Self {
            session_id: session_id.ok_or_else(Self::usage)?,
            snapshot_path,
            sample_interval_ms,
            once,
        })
    }

    fn usage() -> String {
        "usage: desktop_input_agent --session-id <id> [--snapshot-path <path>] [--sample-interval-ms <ms>] [--once]".to_owned()
    }
}

fn capture_desktop_input_snapshot(session_id: u32) -> Option<DesktopInputSnapshot> {
    let (last_input_tick_ms, human_input_quiet_duration_ms) = platform_last_input_state()?;
    Some(DesktopInputSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        source: SNAPSHOT_SOURCE_GET_LAST_INPUT_INFO.to_owned(),
        session_id,
        agent_process_id: std::process::id(),
        last_input_tick_ms,
        human_input_quiet_duration_ms,
        sampled_at_unix_ms: unix_time_ms_now(),
    })
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn platform_last_input_state() -> Option<(u64, u64)> {
    use std::mem::size_of;
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };

    let mut last_input = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    let read_succeeded = unsafe { GetLastInputInfo(&mut last_input) };
    if read_succeeded == 0 {
        return None;
    }
    let now_tick_ms = unsafe { GetTickCount64() as u32 };
    let quiet_ms = now_tick_ms.wrapping_sub(last_input.dwTime);
    Some((u64::from(last_input.dwTime), u64::from(quiet_ms)))
}

#[cfg(windows)]
type PlatformMutexHandle = windows_sys::Win32::Foundation::HANDLE;

#[cfg(windows)]
#[allow(unsafe_code)]
fn platform_acquire_single_instance(session_id: u32) -> SingleInstanceStatus {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    };

    let mutex_name = format!(r"Local\WinFaceUnlockDesktopInputPresenceAgent-{session_id}");
    let wide_name = to_wide_null(&mutex_name);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide_name.as_ptr()) };
    if handle.is_null() {
        return SingleInstanceStatus::Unavailable;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return SingleInstanceStatus::AlreadyRunning;
    }
    SingleInstanceStatus::Acquired(SingleInstanceGuard { raw: handle })
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
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
fn platform_last_input_state() -> Option<(u64, u64)> {
    None
}

#[cfg(not(windows))]
type PlatformMutexHandle = ();

#[cfg(not(windows))]
fn platform_acquire_single_instance(_session_id: u32) -> SingleInstanceStatus {
    SingleInstanceStatus::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_require_session_id() {
        assert!(AgentConfig::from_args(Vec::<String>::new()).is_err());
    }

    #[test]
    fn args_parse_full_config() {
        let config = AgentConfig::from_args([
            "--session-id".to_owned(),
            "3".to_owned(),
            "--snapshot-path".to_owned(),
            r"C:\tmp\state.json".to_owned(),
            "--sample-interval-ms".to_owned(),
            "60000".to_owned(),
            "--once".to_owned(),
        ]);
        let config = match config {
            Ok(config) => config,
            Err(error) => {
                assert!(error.is_empty(), "agent config should parse: {error}");
                return;
            }
        };

        assert_eq!(config.session_id, 3);
        assert_eq!(config.snapshot_path, PathBuf::from(r"C:\tmp\state.json"));
        assert_eq!(config.sample_interval_ms, 60_000);
        assert!(config.once);
    }
}
