use opencv::{
    core::Mat,
    prelude::{MatTraitConst, MatTraitConstManual, VideoCaptureTrait, VideoCaptureTraitConst},
    videoio::{self, VideoCapture},
};

use crate::{CameraId, CameraInfo, PixelFormat, VideoError, VideoFrame, VideoFrameProvider};

const DEFAULT_MAX_CAMERA_INDEX: u32 = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCvCameraProviderConfig {
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
}

impl Default for OpenCvCameraProviderConfig {
    fn default() -> Self {
        Self {
            max_camera_index: DEFAULT_MAX_CAMERA_INDEX,
            requested_frame_width: None,
            requested_frame_height: None,
        }
    }
}

pub struct OpenCvCameraProvider {
    config: OpenCvCameraProviderConfig,
    capture: Option<VideoCapture>,
}

impl OpenCvCameraProvider {
    pub fn new(config: OpenCvCameraProviderConfig) -> Self {
        Self {
            config,
            capture: None,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(OpenCvCameraProviderConfig::default())
    }

    fn try_open_camera(
        camera_index: i32,
        config: &OpenCvCameraProviderConfig,
    ) -> Result<VideoCapture, VideoError> {
        let mut capture = VideoCapture::new(camera_index, videoio::CAP_ANY)
            .map_err(|_| VideoError::OpenFailed)?;
        if let Some(width) = config.requested_frame_width {
            let _ = capture.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(width));
        }
        if let Some(height) = config.requested_frame_height {
            let _ = capture.set(videoio::CAP_PROP_FRAME_HEIGHT, f64::from(height));
        }

        let camera_is_open = capture.is_opened().map_err(|_| VideoError::OpenFailed)?;
        if !camera_is_open {
            return Err(VideoError::OpenFailed);
        }

        Ok(capture)
    }

    fn frame_from_mat(frame: &Mat) -> Result<VideoFrame, VideoError> {
        if frame.empty() || frame.cols() <= 0 || frame.rows() <= 0 {
            return Err(VideoError::EmptyFrame);
        }

        let format = match frame.channels() {
            1 => PixelFormat::Gray8,
            3 => PixelFormat::Bgr8,
            _ => return Err(VideoError::UnsupportedFormat),
        };
        let data = frame
            .data_bytes()
            .map_err(|_| VideoError::ReadFailed)?
            .to_vec();
        let frame = VideoFrame {
            width: frame.cols() as u32,
            height: frame.rows() as u32,
            format,
            data,
        };
        frame.validate()?;
        Ok(frame)
    }
}

impl Default for OpenCvCameraProvider {
    fn default() -> Self {
        Self::with_default_config()
    }
}

impl VideoFrameProvider for OpenCvCameraProvider {
    fn list_sources(&self) -> Result<Vec<CameraInfo>, VideoError> {
        let mut sources = Vec::new();
        for camera_index in 0..=self.config.max_camera_index {
            let Ok(mut capture) = Self::try_open_camera(camera_index as i32, &self.config) else {
                continue;
            };
            sources.push(CameraInfo {
                id: CameraId::from_index(camera_index),
                display_name: format!("Local camera {camera_index}"),
            });
            let _ = capture.release();
        }
        Ok(sources)
    }

    fn open(&mut self, camera_id: &CameraId) -> Result<(), VideoError> {
        if self.capture.is_some() {
            return Err(VideoError::CameraAlreadyOpen);
        }

        self.capture = Some(Self::try_open_camera(
            camera_id.camera_index()?,
            &self.config,
        )?);
        Ok(())
    }

    fn read_frame(&mut self) -> Result<VideoFrame, VideoError> {
        let capture = self.capture.as_mut().ok_or(VideoError::CameraNotOpen)?;
        let mut frame = Mat::default();
        let frame_was_read = capture
            .read(&mut frame)
            .map_err(|_| VideoError::ReadFailed)?;
        if !frame_was_read {
            return Err(VideoError::ReadFailed);
        }

        Self::frame_from_mat(&frame)
    }

    fn close(&mut self) {
        if let Some(mut capture) = self.capture.take() {
            let _ = capture.release();
        }
    }
}

impl Drop for OpenCvCameraProvider {
    fn drop(&mut self) {
        self.close();
    }
}
