//! Windows process-lifetime boundary; closing the job kills abandoned workers.
#![allow(unsafe_code)]
use std::{io, os::windows::io::AsRawHandle, process::Child};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
};

pub(crate) struct WorkerJob(HANDLE);
// The handle is owned and only used through shared Windows kernel APIs.
unsafe impl Send for WorkerJob {}

impl WorkerJob {
    pub(crate) fn attach(child: &Child) -> io::Result<Self> {
        // SAFETY: null security/name create a private, non-inheritable job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: valid owned handles and correctly sized structure. Child waits
        // for our first pipe request before opening any camera.
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
            || unsafe { AssignProcessToJobObject(handle, child.as_raw_handle().cast()) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
}

impl Drop for WorkerJob {
    fn drop(&mut self) {
        // SAFETY: the owned job handle is closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn closing_the_job_terminates_the_worker_without_child_kill() -> io::Result<()> {
        let mut child =
            Command::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "$null = [Console]::OpenStandardInput().ReadByte(); Start-Sleep -Seconds 30",
                ])
                .creation_flags(0x08000000)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        let job = match WorkerJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                return Err(error);
            }
        };
        // This is also the kernel operation when a parent exits: its last
        // private job handle closes, even if no Rust destructor runs.
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
