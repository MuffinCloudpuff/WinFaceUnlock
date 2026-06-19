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
        #[cfg(windows)]
        let backend = videoio::CAP_DSHOW;
        #[cfg(not(windows))]
        let backend = videoio::CAP_ANY;

        let mut capture =
            VideoCapture::new(camera_index, backend).map_err(|_| VideoError::OpenFailed)?;
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

    #[cfg(not(windows))]
    fn list_sources_by_open_probe(&self) -> Result<Vec<CameraInfo>, VideoError> {
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
}

#[cfg(windows)]
impl OpenCvCameraProvider {
    fn list_sources_without_stream_open(&self) -> Result<Vec<CameraInfo>, VideoError> {
        windows_media_foundation_camera_sources()
    }
}

#[cfg(not(windows))]
impl OpenCvCameraProvider {
    fn list_sources_without_stream_open(&self) -> Result<Vec<CameraInfo>, VideoError> {
        self.list_sources_by_open_probe()
    }
}

impl Default for OpenCvCameraProvider {
    fn default() -> Self {
        Self::with_default_config()
    }
}

impl VideoFrameProvider for OpenCvCameraProvider {
    fn list_sources(&self) -> Result<Vec<CameraInfo>, VideoError> {
        self.list_sources_without_stream_open()
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

#[cfg(windows)]
fn windows_media_foundation_camera_sources() -> Result<Vec<CameraInfo>, VideoError> {
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

#[cfg(windows)]
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
