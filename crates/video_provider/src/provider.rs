#[derive(Clone, Debug, Eq, PartialEq)]
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CameraInfo {
    pub id: CameraId,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
}
