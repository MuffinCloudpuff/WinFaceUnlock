#![cfg(windows)]
#![allow(unsafe_code)]

use crate::{CameraId, CameraInfo, VideoError};
use std::ptr;
use windows::Win32::{
    Media::MediaFoundation::{
        IMFActivate, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID, MF_VERSION, MFCreateAttributes,
        MFEnumDeviceSources, MFShutdown, MFStartup,
    },
    System::Com::CoTaskMemFree,
};

struct MediaFoundationRuntime;

impl MediaFoundationRuntime {
    fn start() -> Result<Self, VideoError> {
        // SAFETY: Media Foundation startup/shutdown are process-local initialization calls.
        unsafe { MFStartup(MF_VERSION, 0).map_err(|_| VideoError::ProviderUnavailable)? };
        Ok(Self)
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        // SAFETY: Balances a successful MFStartup in this scope.
        let _ = unsafe { MFShutdown() };
    }
}

pub(crate) fn windows_media_foundation_camera_sources() -> Result<Vec<CameraInfo>, VideoError> {
    let _runtime = MediaFoundationRuntime::start()?;
    let mut attributes = None;
    // SAFETY: MFCreateAttributes initializes an out-parameter Option managed by windows-rs.
    unsafe { MFCreateAttributes(&mut attributes, 1).map_err(|_| VideoError::ProviderUnavailable)? };
    let attributes = attributes.ok_or(VideoError::ProviderUnavailable)?;
    // SAFETY: The attribute keys and GUID values are Media Foundation constants.
    unsafe {
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|_| VideoError::ProviderUnavailable)?;
    }

    let mut activate_ptr: *mut Option<IMFActivate> = ptr::null_mut();
    let mut activate_count = 0_u32;
    // SAFETY: MFEnumDeviceSources fills a CoTaskMem-allocated activation array that is freed below.
    unsafe {
        MFEnumDeviceSources(&attributes, &mut activate_ptr, &mut activate_count)
            .map_err(|_| VideoError::ProviderUnavailable)?;
    }

    let mut sources = Vec::new();
    if !activate_ptr.is_null() {
        // SAFETY: On success MFEnumDeviceSources returns activate_count initialized entries.
        let activates =
            unsafe { std::slice::from_raw_parts(activate_ptr, activate_count as usize) };
        for (index, activate) in activates.iter().enumerate() {
            let display_name = activate
                .as_ref()
                .and_then(|activate| {
                    allocated_mf_string(activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).ok()
                })
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("Local camera {index}"));
            sources.push(CameraInfo {
                id: CameraId::from_index(index as u32),
                display_name,
            });
        }
        // SAFETY: The activation array itself is allocated by MFEnumDeviceSources with CoTaskMem.
        unsafe { CoTaskMemFree(Some(activate_ptr.cast())) };
    }

    Ok(sources)
}

fn allocated_mf_string(
    attributes: &windows::Win32::Media::MediaFoundation::IMFActivate,
    key: &windows::core::GUID,
) -> Result<String, VideoError> {
    use windows::{Win32::System::Com::CoTaskMemFree, core::PWSTR};

    let mut raw = PWSTR::null();
    let mut len = 0_u32;
    // SAFETY: GetAllocatedString writes a null-terminated CoTaskMem string and its length.
    unsafe {
        attributes
            .GetAllocatedString(key, &mut raw, &mut len)
            .map_err(|_| VideoError::ProviderUnavailable)?;
    }
    if raw.is_null() {
        return Ok(String::new());
    }
    // SAFETY: raw points to len UTF-16 code units allocated by Media Foundation.
    let value =
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw.0, len as usize) });
    // SAFETY: raw is allocated by GetAllocatedString and must be freed with CoTaskMemFree.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    Ok(value)
}
