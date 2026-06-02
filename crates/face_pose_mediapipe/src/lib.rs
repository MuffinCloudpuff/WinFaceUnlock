#![cfg_attr(feature = "native-bridge", allow(unsafe_code))]

use std::path::PathBuf;

#[cfg(feature = "native-bridge")]
use std::{
    ffi::{CString, c_char, c_void},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::NonNull,
};

use face_engine::DetectedFace;
use face_pose::{FacePoseCapabilities, FacePoseError, FacePoseEstimate, FacePoseProvider};
#[cfg(feature = "native-bridge")]
use video_provider::PixelFormat;
use video_provider::VideoFrame;

#[cfg(feature = "native-bridge")]
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HMODULE},
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
};

#[cfg(feature = "native-bridge")]
type CreateProviderFn =
    unsafe extern "C" fn(model_path: *const c_char, options: MediaPipeBridgeOptions) -> *mut c_void;
#[cfg(feature = "native-bridge")]
type DestroyProviderFn = unsafe extern "C" fn(provider: *mut c_void);
#[cfg(feature = "native-bridge")]
type EstimatePoseFn = unsafe extern "C" fn(
    provider: *mut c_void,
    request: *const MediaPipeBridgeFrameRequest,
    result: *mut MediaPipeBridgePoseResult,
) -> i32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaPipeFacePoseProviderConfig {
    pub bridge_dll_path: PathBuf,
    pub face_landmarker_task_path: PathBuf,
    pub running_mode: MediaPipeRunningMode,
    pub output_face_blendshapes: bool,
    pub output_facial_transformation_matrixes: bool,
}

impl MediaPipeFacePoseProviderConfig {
    pub fn new(bridge_dll_path: PathBuf, face_landmarker_task_path: PathBuf) -> Self {
        Self {
            bridge_dll_path,
            face_landmarker_task_path,
            running_mode: MediaPipeRunningMode::Image,
            output_face_blendshapes: true,
            output_facial_transformation_matrixes: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaPipeRunningMode {
    Image,
    Video,
}

#[derive(Debug, Eq, PartialEq)]
pub enum MediaPipeFacePoseProviderError {
    NativeBridgeFeatureDisabled,
    BridgePathMissing,
    ModelPathMissing,
    InvalidPath,
    BridgeLoadFailed,
    SymbolLoadFailed,
    ProviderCreateFailed,
}

#[cfg(feature = "native-bridge")]
pub struct MediaPipeFacePoseProvider {
    library: HMODULE,
    provider_handle: NonNull<c_void>,
    destroy_provider: DestroyProviderFn,
    estimate_pose: EstimatePoseFn,
}

#[cfg(not(feature = "native-bridge"))]
pub struct MediaPipeFacePoseProvider;

impl MediaPipeFacePoseProvider {
    #[cfg(feature = "native-bridge")]
    pub fn load(
        config: MediaPipeFacePoseProviderConfig,
    ) -> Result<Self, MediaPipeFacePoseProviderError> {
        if !config.bridge_dll_path.exists() {
            return Err(MediaPipeFacePoseProviderError::BridgePathMissing);
        }
        if !config.face_landmarker_task_path.exists() {
            return Err(MediaPipeFacePoseProviderError::ModelPathMissing);
        }

        let model_path = path_to_c_string(&config.face_landmarker_task_path)?;
        let library = load_bridge_library(&config.bridge_dll_path)?;
        let create_provider =
            load_symbol::<CreateProviderFn>(library, b"winfaceunlock_mediapipe_pose_create\0")?;
        let destroy_provider =
            load_symbol::<DestroyProviderFn>(library, b"winfaceunlock_mediapipe_pose_destroy\0")?;
        let estimate_pose =
            load_symbol::<EstimatePoseFn>(library, b"winfaceunlock_mediapipe_pose_estimate\0")?;

        let options = MediaPipeBridgeOptions {
            running_mode: match config.running_mode {
                MediaPipeRunningMode::Image => 0,
                MediaPipeRunningMode::Video => 1,
            },
            output_face_blendshapes: u8::from(config.output_face_blendshapes),
            output_facial_transformation_matrixes: u8::from(
                config.output_facial_transformation_matrixes,
            ),
            _reserved: [0; 6],
        };
        let provider_handle = unsafe { create_provider(model_path.as_ptr(), options) };
        let provider_handle = NonNull::new(provider_handle)
            .ok_or(MediaPipeFacePoseProviderError::ProviderCreateFailed)?;

        Ok(Self {
            library,
            provider_handle,
            destroy_provider,
            estimate_pose,
        })
    }

    #[cfg(not(feature = "native-bridge"))]
    pub fn load(
        _config: MediaPipeFacePoseProviderConfig,
    ) -> Result<Self, MediaPipeFacePoseProviderError> {
        Err(MediaPipeFacePoseProviderError::NativeBridgeFeatureDisabled)
    }
}

#[cfg(feature = "native-bridge")]
impl Drop for MediaPipeFacePoseProvider {
    fn drop(&mut self) {
        unsafe {
            (self.destroy_provider)(self.provider_handle.as_ptr());
            FreeLibrary(self.library);
        }
    }
}

#[cfg(feature = "native-bridge")]
impl FacePoseProvider for MediaPipeFacePoseProvider {
    fn provider_name(&self) -> &'static str {
        "mediapipe"
    }

    fn capabilities(&self) -> FacePoseCapabilities {
        FacePoseCapabilities::HEAD_POSE_AND_BLINK
    }

    fn estimate_pose(
        &mut self,
        frame: &VideoFrame,
        face: &DetectedFace,
    ) -> Result<FacePoseEstimate, FacePoseError> {
        frame
            .validate()
            .map_err(|_| FacePoseError::InferenceFailed)?;
        let pixel_format = match frame.format {
            PixelFormat::Bgr8 => 0,
            PixelFormat::Rgb8 => 1,
            PixelFormat::Gray8 => 2,
        };
        let request = MediaPipeBridgeFrameRequest {
            width: frame.width,
            height: frame.height,
            pixel_format,
            _reserved: 0,
            data: frame.data.as_ptr(),
            data_len: frame.data.len(),
            face_box_x: face.bounds.x,
            face_box_y: face.bounds.y,
            face_box_width: face.bounds.width,
            face_box_height: face.bounds.height,
        };
        let mut result = MediaPipeBridgePoseResult::default();
        let status =
            unsafe { (self.estimate_pose)(self.provider_handle.as_ptr(), &request, &mut result) };
        if status != 0 {
            return Err(FacePoseError::InferenceFailed);
        }

        Ok(FacePoseEstimate::with_blink_scores(
            result.yaw_deg,
            result.pitch_deg,
            result.roll_deg,
            result.left_eye_blink_score,
            result.right_eye_blink_score,
        ))
    }
}

#[cfg(not(feature = "native-bridge"))]
impl FacePoseProvider for MediaPipeFacePoseProvider {
    fn provider_name(&self) -> &'static str {
        "mediapipe"
    }

    fn capabilities(&self) -> FacePoseCapabilities {
        FacePoseCapabilities::HEAD_POSE_AND_BLINK
    }

    fn estimate_pose(
        &mut self,
        _frame: &VideoFrame,
        _face: &DetectedFace,
    ) -> Result<FacePoseEstimate, FacePoseError> {
        Err(FacePoseError::ProviderUnavailable)
    }
}

#[cfg(feature = "native-bridge")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MediaPipeBridgeOptions {
    running_mode: u32,
    output_face_blendshapes: u8,
    output_facial_transformation_matrixes: u8,
    _reserved: [u8; 6],
}

#[cfg(feature = "native-bridge")]
#[repr(C)]
struct MediaPipeBridgeFrameRequest {
    width: u32,
    height: u32,
    pixel_format: u32,
    _reserved: u32,
    data: *const u8,
    data_len: usize,
    face_box_x: f32,
    face_box_y: f32,
    face_box_width: f32,
    face_box_height: f32,
}

#[cfg(feature = "native-bridge")]
#[repr(C)]
#[derive(Default)]
struct MediaPipeBridgePoseResult {
    yaw_deg: f32,
    pitch_deg: f32,
    roll_deg: f32,
    left_eye_blink_score: f32,
    right_eye_blink_score: f32,
}

#[cfg(feature = "native-bridge")]
fn path_to_c_string(path: &Path) -> Result<CString, MediaPipeFacePoseProviderError> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| MediaPipeFacePoseProviderError::InvalidPath)
}

#[cfg(feature = "native-bridge")]
fn load_bridge_library(path: &Path) -> Result<HMODULE, MediaPipeFacePoseProviderError> {
    let path_wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let library = unsafe { LoadLibraryW(path_wide.as_ptr()) };
    if library.is_null() {
        return Err(MediaPipeFacePoseProviderError::BridgeLoadFailed);
    }
    Ok(library)
}

#[cfg(feature = "native-bridge")]
fn load_symbol<T>(library: HMODULE, symbol_name: &[u8]) -> Result<T, MediaPipeFacePoseProviderError>
where
    T: Copy,
{
    let symbol = unsafe { GetProcAddress(library, symbol_name.as_ptr()) }
        .ok_or(MediaPipeFacePoseProviderError::SymbolLoadFailed)?;
    Ok(unsafe { std::mem::transmute_copy(&symbol) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_requests_pose_and_blink_outputs() {
        let config = MediaPipeFacePoseProviderConfig::new(
            PathBuf::from("bridge.dll"),
            PathBuf::from("face_landmarker.task"),
        );

        assert!(config.output_face_blendshapes);
        assert!(config.output_facial_transformation_matrixes);
        assert_eq!(config.running_mode, MediaPipeRunningMode::Image);
    }

    #[cfg(not(feature = "native-bridge"))]
    #[test]
    fn default_build_reports_disabled_native_bridge() {
        let config = MediaPipeFacePoseProviderConfig::new(
            PathBuf::from("bridge.dll"),
            PathBuf::from("face_landmarker.task"),
        );

        assert!(matches!(
            MediaPipeFacePoseProvider::load(config),
            Err(MediaPipeFacePoseProviderError::NativeBridgeFeatureDisabled)
        ));
    }
}
