//! Windows scheduling adapter. Startup order remains owned by SCM configuration.
#![allow(unsafe_code)]

pub(crate) fn raise_service_priority() -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        ABOVE_NORMAL_PRIORITY_CLASS, GetCurrentProcess, SetPriorityClass,
    };

    // SAFETY: The pseudo-handle refers to this process and needs no closing.
    // AboveNormal gives the broker a modest boost without using High/Realtime.
    if unsafe { SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
