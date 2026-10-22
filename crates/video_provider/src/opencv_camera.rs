use opencv::{
    core::Mat,
    prelude::{MatTraitConst, MatTraitConstManual, VideoCaptureTrait, VideoCaptureTraitConst},
    videoio::{self, VideoCapture},
};
use serde::{Deserialize, Serialize};

use crate::{CameraId, CameraInfo, PixelFormat, VideoError, VideoFrame, VideoFrameProvider};

const DEFAULT_MAX_CAMERA_INDEX: u32 = 8;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenCvCameraProviderConfig {
    pub max_camera_index: u32,
    pub requested_frame_width: Option<u32>,
    pub requested_frame_height: Option<u32>,
    pub preferred_backend: Option<OpenCvCameraBackend>,
}

impl Default for OpenCvCameraProviderConfig {
    fn default() -> Self {
        Self {
            max_camera_index: DEFAULT_MAX_CAMERA_INDEX,
            requested_frame_width: None,
            requested_frame_height: None,
            preferred_backend: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenCvCameraBackend {
    Msmf,
    Dshow,
    Any,
}

impl OpenCvCameraBackend {
    pub fn all() -> [Self; 3] {
        [Self::Msmf, Self::Dshow, Self::Any]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Msmf => "msmf",
            Self::Dshow => "dshow",
            Self::Any => "any",
        }
    }

    pub fn videoio_id(self) -> i32 {
        match self {
            Self::Msmf => videoio::CAP_MSMF,
            Self::Dshow => videoio::CAP_DSHOW,
            Self::Any => videoio::CAP_ANY,
        }
    }
}

pub struct InProcessCameraProvider {
    config: OpenCvCameraProviderConfig,
    capture: Option<VideoCapture>,
    first_frame: Option<VideoFrame>,
}

impl InProcessCameraProvider {
    pub fn new(config: OpenCvCameraProviderConfig) -> Self {
        Self {
            config,
            capture: None,
            first_frame: None,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(OpenCvCameraProviderConfig::default())
    }

    fn try_open_camera(
        camera_id: &CameraId,
        config: &OpenCvCameraProviderConfig,
        exact_backend: Option<OpenCvCameraBackend>,
    ) -> Result<(VideoCapture, VideoFrame), VideoError> {
        let camera_id = crate::resolve_camera_id(camera_id)?;
        #[cfg(windows)]
        let backends = exact_backend
            .map(|backend| vec![backend])
            .unwrap_or_else(|| {
                crate::windows_camera_backend_order(config.preferred_backend).to_vec()
            });
        #[cfg(not(windows))]
        let backends = [exact_backend.unwrap_or(OpenCvCameraBackend::Any)];

        crate::capture_open::open_readable_capture(
            backends,
            |backend| {
                let camera_index =
                    crate::camera_identity::camera_index_for_backend(&camera_id, backend)?;
                let mut capture = VideoCapture::new(camera_index, backend.videoio_id())
                    .map_err(|_| VideoError::OpenFailed)?;
                if !capture.is_opened().unwrap_or(false) {
                    return Err(VideoError::OpenFailed);
                }
                if let Some(width) = config.requested_frame_width {
                    let _ = capture.set(videoio::CAP_PROP_FRAME_WIDTH, f64::from(width));
                }
                if let Some(height) = config.requested_frame_height {
                    let _ = capture.set(videoio::CAP_PROP_FRAME_HEIGHT, f64::from(height));
                }

                // Reject a hotplug/reordering observed while opening rather than
                // continuing with an index that now denotes a different device.
                if crate::camera_identity::camera_index_for_backend(&camera_id, backend)?
                    != camera_index
                {
                    return Err(VideoError::CameraNotFound);
                }

                Ok(capture)
            },
            Self::read_capture_frame,
            || std::thread::sleep(std::time::Duration::from_millis(50)),
        )
    }

    /// Opens exactly one backend for profiling, with no hidden fallback.
    pub fn open_backend(
        &mut self,
        camera_id: &CameraId,
        backend: OpenCvCameraBackend,
    ) -> Result<(), VideoError> {
        self.open_with_backend(camera_id, Some(backend))
    }

    fn open_with_backend(
        &mut self,
        camera_id: &CameraId,
        backend: Option<OpenCvCameraBackend>,
    ) -> Result<(), VideoError> {
        if self.capture.is_some() {
            return Err(VideoError::CameraAlreadyOpen);
        }
        let (capture, first_frame) = Self::try_open_camera(camera_id, &self.config, backend)?;
        self.capture = Some(capture);
        self.first_frame = Some(first_frame);
        Ok(())
    }

    fn read_capture_frame(capture: &mut VideoCapture) -> Result<VideoFrame, VideoError> {
        let mut frame = Mat::default();
        if !capture
            .read(&mut frame)
            .map_err(|_| VideoError::ReadFailed)?
        {
            return Err(VideoError::ReadFailed);
        }
        Self::frame_from_mat(&frame)
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

    #[cfg(not(windows))]
    fn list_sources_by_open_probe(&self) -> Result<Vec<CameraInfo>, VideoError> {
        let mut sources = Vec::new();
        for camera_index in 0..=self.config.max_camera_index {
            let Ok((mut capture, _)) =
                Self::try_open_camera(&CameraId::from_index(camera_index), &self.config, None)
            else {
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
}

#[cfg(windows)]
impl InProcessCameraProvider {
    fn list_sources_without_stream_open(&self) -> Result<Vec<CameraInfo>, VideoError> {
        crate::list_camera_sources()
    }
}

#[cfg(not(windows))]
impl InProcessCameraProvider {
    fn list_sources_without_stream_open(&self) -> Result<Vec<CameraInfo>, VideoError> {
        self.list_sources_by_open_probe()
    }
}

impl Default for InProcessCameraProvider {
    fn default() -> Self {
        Self::with_default_config()
    }
}

impl VideoFrameProvider for InProcessCameraProvider {
    fn list_sources(&self) -> Result<Vec<CameraInfo>, VideoError> {
        self.list_sources_without_stream_open()
    }

    fn open(&mut self, camera_id: &CameraId) -> Result<(), VideoError> {
        self.open_with_backend(camera_id, None)
    }

    fn read_frame(&mut self) -> Result<VideoFrame, VideoError> {
        if let Some(frame) = self.first_frame.take() {
            return Ok(frame);
        }
        let capture = self.capture.as_mut().ok_or(VideoError::CameraNotOpen)?;
        Self::read_capture_frame(capture)
    }

    fn close(&mut self) {
        self.first_frame = None;
        if let Some(mut capture) = self.capture.take() {
            let _ = capture.release();
        }
    }
}

impl Drop for InProcessCameraProvider {
    fn drop(&mut self) {
        self.close();
    }
}
