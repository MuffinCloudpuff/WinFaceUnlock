use serde::{Deserialize, Serialize};

const DEVICE_ID_PREFIX: &str = "windows-device:";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CameraId(pub String);

impl CameraId {
    pub fn from_index(camera_index: u32) -> Self {
        Self(format!("opencv-index:{camera_index}"))
    }

    pub fn camera_index(&self) -> Result<i32, VideoError> {
        let index = self
            .0
            .strip_prefix("opencv-index:")
            .ok_or(VideoError::CameraNotFound)?
            .parse::<u32>()
            .map_err(|_| VideoError::CameraNotFound)?;

        i32::try_from(index).map_err(|_| VideoError::CameraNotFound)
    }

    /// A Windows capture interface path identifies a device without relying on
    /// its friendly name or its position in either backend's enumeration.
    pub fn from_device_path(path: &str) -> Result<Self, VideoError> {
        let path = path.strip_prefix("@device:pnp:").unwrap_or(path);
        if !path.starts_with(r"\\?\")
            || path.len() <= 4
            || path.len() > 8192
            || path.chars().any(char::is_control)
        {
            return Err(VideoError::CameraNotFound);
        }
        Ok(Self(format!(
            "{DEVICE_ID_PREFIX}{}",
            path.to_ascii_lowercase()
        )))
    }

    pub fn is_stable(&self) -> bool {
        self.0.starts_with(DEVICE_ID_PREFIX) && self.validate().is_ok()
    }

    /// Syntax-only validation; configuration parsing must not access hardware.
    pub fn validate(&self) -> Result<(), VideoError> {
        if let Some(path) = self.0.strip_prefix(DEVICE_ID_PREFIX) {
            Self::from_device_path(path).map(|_| ())
        } else {
            self.camera_index().map(|_| ())
        }
    }

    pub(crate) fn normalized(&self) -> Result<Self, VideoError> {
        if let Some(path) = self.0.strip_prefix(DEVICE_ID_PREFIX) {
            Self::from_device_path(path)
        } else {
            Ok(Self::from_index(self.camera_index()? as u32))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CameraInfo {
    pub id: CameraId,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum VideoError {
    ProviderUnavailable,
    CameraNotFound,
    CameraAlreadyOpen,
    CameraNotOpen,
    OpenFailed,
    ReadFailed,
    EmptyFrame,
    UnsupportedFormat,
}

pub trait VideoFrameProvider {
    fn list_sources(&self) -> Result<Vec<CameraInfo>, VideoError>;
    fn open(&mut self, camera_id: &CameraId) -> Result<(), VideoError>;
    fn read_frame(&mut self) -> Result<crate::VideoFrame, VideoError>;
    fn close(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_id_round_trips_opencv_index() -> Result<(), VideoError> {
        let camera_id = CameraId::from_index(3);

        assert_eq!(camera_id.camera_index()?, 3);
        Ok(())
    }

    #[test]
    fn device_path_identity_is_case_insensitive_and_not_an_index() -> Result<(), VideoError> {
        let id = CameraId::from_device_path(r"\\?\USB#VID_ABCD#serial#{interface}\global")?;
        assert_eq!(
            id,
            CameraId::from_device_path(r"\\?\usb#vid_abcd#serial#{interface}\global")?
        );
        assert!(id.is_stable());
        assert_eq!(id.camera_index(), Err(VideoError::CameraNotFound));
        Ok(())
    }

    #[test]
    fn malformed_ids_and_display_names_are_not_device_identities() {
        for value in [
            "windows-device:",
            "windows-device:USB Camera",
            "opencv-index:-1",
            "opencv-index:4294967295",
        ] {
            assert!(CameraId(value.to_owned()).validate().is_err());
        }
        assert!(CameraId::from_device_path("USB Camera").is_err());
        assert!(CameraId::from_device_path("\\\\?\\usb\ninvalid").is_err());
    }

    #[test]
    fn normalization_preserves_distinct_device_reference_strings() -> Result<(), VideoError> {
        let first = CameraId::from_device_path(r"\\?\USB#camera#{interface}\rgb")?;
        let second = CameraId::from_device_path(r"\\?\USB#camera#{interface}\ir")?;
        assert_ne!(first, second);
        assert_eq!(
            first,
            CameraId::from_device_path(r"@device:pnp:\\?\usb#camera#{interface}\RGB")?
        );
        Ok(())
    }
}
