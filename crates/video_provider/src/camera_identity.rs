//! Device identity and backend-local index resolution. Enumeration never opens a stream.
use crate::{CameraId, CameraInfo, OpenCvCameraBackend, VideoError};

#[derive(Clone, Debug)]
pub(crate) struct CameraDevice {
    pub index: i32,
    pub id: Option<CameraId>,
    pub display_name: String,
}

/// Bind an old OpenCV index to the current Media Foundation device once, then
/// persist the returned ID. A legacy index cannot recover historical identity.
/// Already-bound IDs are checked by identity and never matched by display name.
pub fn resolve_camera_id(camera_id: &CameraId) -> Result<CameraId, VideoError> {
    camera_id.validate()?;
    #[cfg(windows)]
    {
        bind_from_devices(
            camera_id,
            &crate::mf_bindings::windows_media_foundation_devices()?,
        )
    }
    #[cfg(not(windows))]
    {
        camera_id.camera_index()?;
        camera_id.normalized()
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn bind_from_devices(
    requested: &CameraId,
    devices: &[CameraDevice],
) -> Result<CameraId, VideoError> {
    let requested = requested.normalized()?;
    if requested.is_stable() {
        unique_device_index(&requested, devices)?;
        return Ok(requested);
    }
    let index = requested.camera_index()?;
    let id = devices
        .iter()
        .find(|device| device.index == index)
        .and_then(|device| device.id.clone())
        .ok_or(VideoError::CameraNotFound)?;
    unique_device_index(&id, devices)?;
    Ok(id)
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn unique_device_index(
    id: &CameraId,
    devices: &[CameraDevice],
) -> Result<i32, VideoError> {
    let mut matches = devices
        .iter()
        .filter(|device| device.id.as_ref() == Some(id));
    let first = matches.next().ok_or(VideoError::CameraNotFound)?;
    if matches.next().is_some() {
        return Err(VideoError::CameraNotFound);
    }
    Ok(first.index)
}

#[cfg(windows)]
pub(crate) fn camera_index_for_backend(
    id: &CameraId,
    backend: OpenCvCameraBackend,
) -> Result<i32, VideoError> {
    let devices = match backend {
        OpenCvCameraBackend::Msmf => crate::mf_bindings::windows_media_foundation_devices()?,
        OpenCvCameraBackend::Dshow => crate::dshow_bindings::windows_directshow_devices()?,
        // CAP_ANY can choose a backend with a different enumeration. Never pass
        // a bound camera to an unspecified backend.
        OpenCvCameraBackend::Any => return Err(VideoError::CameraNotFound),
    };
    unique_device_index(&id.normalized()?, &devices)
}

#[cfg(not(windows))]
pub(crate) fn camera_index_for_backend(
    id: &CameraId,
    _backend: OpenCvCameraBackend,
) -> Result<i32, VideoError> {
    id.camera_index()
}

#[cfg(windows)]
pub fn list_camera_sources() -> Result<Vec<CameraInfo>, VideoError> {
    let devices = crate::mf_bindings::windows_media_foundation_devices()?;
    let mut identity_counts = std::collections::HashMap::new();
    for device in &devices {
        if let Some(id) = &device.id {
            *identity_counts.entry(id.0.as_str()).or_insert(0_usize) += 1;
        }
    }
    Ok(devices
        .iter()
        .filter_map(|device| {
            let id = device.id.as_ref()?;
            if identity_counts.get(id.0.as_str()) != Some(&1) {
                return None;
            }
            Some(CameraInfo {
                id: id.clone(),
                display_name: device.display_name.clone(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(index: i32, path: &str) -> Result<CameraDevice, VideoError> {
        Ok(CameraDevice {
            index,
            id: Some(CameraId::from_device_path(path)?),
            display_name: "Identical USB Camera".to_owned(),
        })
    }

    #[test]
    fn reordered_backends_resolve_the_same_physical_device() -> Result<(), VideoError> {
        let mf = [device(0, r"\\?\usb#one")?, device(1, r"\\?\usb#two")?];
        let dshow = [device(0, r"\\?\usb#two")?, device(1, r"\\?\usb#one")?];
        let id = bind_from_devices(&CameraId::from_index(0), &mf)?;
        assert_eq!(unique_device_index(&id, &mf)?, 0);
        assert_eq!(unique_device_index(&id, &dshow)?, 1);
        assert_eq!(bind_from_devices(&id, &dshow)?, id);
        Ok(())
    }

    #[test]
    fn disappearance_never_selects_an_identically_named_device() -> Result<(), VideoError> {
        let id = CameraId::from_device_path(r"\\?\usb#one")?;
        assert_eq!(
            bind_from_devices(&id, &[device(0, r"\\?\usb#two")?]),
            Err(VideoError::CameraNotFound)
        );
        Ok(())
    }

    #[test]
    fn duplicate_or_missing_identity_fails_closed() -> Result<(), VideoError> {
        let id = CameraId::from_device_path(r"\\?\usb#one")?;
        let duplicate = [device(0, r"\\?\usb#one")?, device(1, r"\\?\usb#one")?];
        assert_eq!(
            unique_device_index(&id, &duplicate),
            Err(VideoError::CameraNotFound)
        );
        let missing = [CameraDevice {
            index: 0,
            id: None,
            display_name: "Camera".to_owned(),
        }];
        assert_eq!(
            bind_from_devices(&CameraId::from_index(0), &missing),
            Err(VideoError::CameraNotFound)
        );
        Ok(())
    }
}
