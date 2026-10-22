//! Resolve capture-interface aliases without parsing Windows' opaque paths.
#![cfg(windows)]
#![allow(unsafe_code)]

use crate::{CameraId, VideoError};
use windows::{
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            HDEVINFO, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
            SetupDiCreateDeviceInfoList, SetupDiDestroyDeviceInfoList,
            SetupDiGetDeviceInterfaceAlias, SetupDiGetDeviceInterfaceDetailW,
            SetupDiOpenDeviceInterfaceW,
        },
        Media::KernelStreaming::KSCATEGORY_VIDEO_CAMERA,
    },
    core::PCWSTR,
};

struct DeviceInfoSet(HDEVINFO);

impl Drop for DeviceInfoSet {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the successfully created info set.
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

/// MF and DirectShow may expose different interface classes for one camera.
/// SetupAPI aliases require the same device *and* reference string, preserving
/// multiple camera endpoints on a composite device without name heuristics.
pub(crate) fn directshow_camera_id(path: &str) -> Result<CameraId, VideoError> {
    let path = path.strip_prefix("@device:pnp:").unwrap_or(path);
    let exact_id = CameraId::from_device_path(path)?;
    Ok(media_foundation_alias(path).unwrap_or(exact_id))
}

fn media_foundation_alias(path: &str) -> Result<CameraId, VideoError> {
    // SAFETY: no class/parent restricts this temporary metadata-only info set.
    let set = DeviceInfoSet(
        unsafe { SetupDiCreateDeviceInfoList(None, None) }
            .map_err(|_| VideoError::CameraNotFound)?,
    );
    let path_utf16: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
    let mut source = SP_DEVICE_INTERFACE_DATA {
        cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
        ..Default::default()
    };
    // SAFETY: path is terminated, and source is a correctly sized output.
    unsafe {
        SetupDiOpenDeviceInterfaceW(set.0, PCWSTR(path_utf16.as_ptr()), 0, Some(&mut source))
    }
    .map_err(|_| VideoError::CameraNotFound)?;
    let mut alias = SP_DEVICE_INTERFACE_DATA {
        cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
        ..Default::default()
    };
    // SAFETY: source belongs to this set; alias output and class GUID are valid.
    unsafe { SetupDiGetDeviceInterfaceAlias(set.0, &source, &KSCATEGORY_VIDEO_CAMERA, &mut alias) }
        .map_err(|_| VideoError::CameraNotFound)?;
    let mut required = 0;
    // SAFETY: the first call asks only for the buffer size; insufficient-buffer
    // is expected and the required byte count is validated before allocation.
    let _ = unsafe {
        SetupDiGetDeviceInterfaceDetailW(set.0, &alias, None, 0, Some(&mut required), None)
    };
    if required < std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32
        || required > 32_768
    {
        return Err(VideoError::CameraNotFound);
    }
    // u64 storage provides alignment for the ABI struct on both Windows targets.
    let mut storage = vec![0_u64; (required as usize).div_ceil(8)];
    let detail = storage
        .as_mut_ptr()
        .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
    // SAFETY: aligned initialized buffer is at least required bytes long.
    unsafe { (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
    // SAFETY: writes within the supplied buffer; all referenced metadata lives.
    unsafe { SetupDiGetDeviceInterfaceDetailW(set.0, &alias, Some(detail), required, None, None) }
        .map_err(|_| VideoError::CameraNotFound)?;
    let offset = std::mem::offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
    let units = (required as usize - offset) / 2;
    // SAFETY: DevicePath lies in the initialized allocation and units is bounded
    // by SetupAPI's byte count; read only through the first terminating NUL.
    let raw = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*detail).DevicePath).cast::<u16>(),
            units,
        )
    };
    let end = raw
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(VideoError::CameraNotFound)?;
    CameraId::from_device_path(&String::from_utf16_lossy(&raw[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregistered_paths_are_not_aliased_by_string_rewriting() -> Result<(), VideoError> {
        let dshow =
            r"\\?\winfaceunlock_nonexistent#fixture#{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global";
        let mf =
            r"\\?\winfaceunlock_nonexistent#fixture#{e5323777-f976-4f5b-9b55-b94699c46e44}\global";
        let resolved = directshow_camera_id(dshow)?;
        assert_eq!(resolved, CameraId::from_device_path(dshow)?);
        assert_ne!(resolved, CameraId::from_device_path(mf)?);
        Ok(())
    }

    #[test]
    #[ignore = "Reads installed camera metadata; run explicitly without opening streams"]
    fn camera_metadata_enumerates_on_a_fresh_thread() -> Result<(), VideoError> {
        std::thread::spawn(|| {
            let started = std::time::Instant::now();
            let mf = crate::mf_bindings::windows_media_foundation_devices()?;
            let dshow = crate::dshow_bindings::windows_directshow_devices()?;
            println!(
                "metadata_elapsed_ms={} mf_count={} dshow_count={}",
                started.elapsed().as_millis(),
                mf.len(),
                dshow.len()
            );
            for device in &mf {
                let dshow_index = device
                    .id
                    .as_ref()
                    .and_then(|id| crate::camera_identity::unique_device_index(id, &dshow).ok());
                println!(
                    "mf_index={} dshow_index={dshow_index:?} name={} id={:?}",
                    device.index, device.display_name, device.id
                );
            }
            Ok(())
        })
        .join()
        .map_err(|_| VideoError::ProviderUnavailable)?
    }
}
