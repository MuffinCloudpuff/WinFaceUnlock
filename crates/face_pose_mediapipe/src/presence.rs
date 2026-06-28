use std::path::PathBuf;

use video_provider::VideoFrame;

use crate::MediaPipeRunningMode;

#[cfg(feature = "native-bridge")]
use crate::{
    MediaPipeBridgeFrameRequest, load_bridge_library, load_symbol, path_to_c_string,
    pixel_format_code,
};

#[cfg(feature = "native-bridge")]
use std::{
    ffi::{c_char, c_void},
    ptr::NonNull,
};

#[cfg(feature = "native-bridge")]
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};

#[cfg(feature = "native-bridge")]
type CreatePresencePoseFn = unsafe extern "C" fn(
    model_path: *const c_char,
    options: MediaPipePresencePoseBridgeOptions,
) -> *mut c_void;
#[cfg(feature = "native-bridge")]
type DestroyPresencePoseFn = unsafe extern "C" fn(provider: *mut c_void);
#[cfg(feature = "native-bridge")]
type EstimatePresencePoseFn = unsafe extern "C" fn(
    provider: *mut c_void,
    request: *const MediaPipeBridgeFrameRequest,
    result: *mut MediaPipePresencePoseBridgeResult,
) -> i32;

#[derive(Clone, Debug, PartialEq)]
pub struct MediaPipePresencePoseProviderConfig {
    pub bridge_dll_path: PathBuf,
    pub pose_landmarker_task_path: PathBuf,
    pub running_mode: MediaPipeRunningMode,
    pub min_landmark_visibility: f32,
    pub min_landmark_presence: f32,
}

impl MediaPipePresencePoseProviderConfig {
    pub fn new(bridge_dll_path: PathBuf, pose_landmarker_task_path: PathBuf) -> Self {
        Self {
            bridge_dll_path,
            pose_landmarker_task_path,
            running_mode: MediaPipeRunningMode::Image,
            min_landmark_visibility: 0.45,
            min_landmark_presence: 0.45,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaPipePresencePoseEstimate {
    pub confidence: f32,
    pub bbox_center_x_ratio: f32,
    pub bbox_area_ratio: f32,
    pub normalized_x_min: f32,
    pub normalized_y_min: f32,
    pub normalized_x_max: f32,
    pub normalized_y_max: f32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum MediaPipePresencePoseProviderError {
    NativeBridgeFeatureDisabled,
    BridgePathMissing,
    ModelPathMissing,
    InvalidPath,
    BridgeLoadFailed,
    SymbolLoadFailed,
    ProviderCreateFailed,
    InvalidFrame,
    InferenceFailed,
}

#[cfg(feature = "native-bridge")]
pub struct MediaPipePresencePoseProvider {
    library: HMODULE,
    provider_handle: NonNull<c_void>,
    destroy_provider: DestroyPresencePoseFn,
    estimate_pose: EstimatePresencePoseFn,
}

#[cfg(not(feature = "native-bridge"))]
pub struct MediaPipePresencePoseProvider;

impl MediaPipePresencePoseProvider {
    #[cfg(feature = "native-bridge")]
    pub fn load(
        config: MediaPipePresencePoseProviderConfig,
    ) -> Result<Self, MediaPipePresencePoseProviderError> {
        if !config.bridge_dll_path.exists() {
            return Err(MediaPipePresencePoseProviderError::BridgePathMissing);
        }
        if !config.pose_landmarker_task_path.exists() {
            return Err(MediaPipePresencePoseProviderError::ModelPathMissing);
        }

        let model_path = path_to_c_string(&config.pose_landmarker_task_path)
            .map_err(|_| MediaPipePresencePoseProviderError::InvalidPath)?;
        let library =
            load_bridge_library(&config.bridge_dll_path).map_err(|error| match error {
                crate::MediaPipeFacePoseProviderError::BridgeLoadFailed => {
                    MediaPipePresencePoseProviderError::BridgeLoadFailed
                }
                _ => MediaPipePresencePoseProviderError::InvalidPath,
            })?;
        let create_provider = load_symbol::<CreatePresencePoseFn>(
            library,
            b"winfaceunlock_mediapipe_presence_pose_create\0",
        )
        .map_err(|_| MediaPipePresencePoseProviderError::SymbolLoadFailed)?;
        let destroy_provider = load_symbol::<DestroyPresencePoseFn>(
            library,
            b"winfaceunlock_mediapipe_presence_pose_destroy\0",
        )
        .map_err(|_| MediaPipePresencePoseProviderError::SymbolLoadFailed)?;
        let estimate_pose = load_symbol::<EstimatePresencePoseFn>(
            library,
            b"winfaceunlock_mediapipe_presence_pose_estimate\0",
        )
        .map_err(|_| MediaPipePresencePoseProviderError::SymbolLoadFailed)?;

        let options = MediaPipePresencePoseBridgeOptions {
            running_mode: match config.running_mode {
                MediaPipeRunningMode::Image => 0,
                MediaPipeRunningMode::Video => 1,
            },
            min_landmark_visibility: config.min_landmark_visibility,
            min_landmark_presence: config.min_landmark_presence,
        };
        let provider_handle = unsafe { create_provider(model_path.as_ptr(), options) };
        let provider_handle = NonNull::new(provider_handle)
            .ok_or(MediaPipePresencePoseProviderError::ProviderCreateFailed)?;

        Ok(Self {
            library,
            provider_handle,
            destroy_provider,
            estimate_pose,
        })
    }

    #[cfg(not(feature = "native-bridge"))]
    pub fn load(
        _config: MediaPipePresencePoseProviderConfig,
    ) -> Result<Self, MediaPipePresencePoseProviderError> {
        Err(MediaPipePresencePoseProviderError::NativeBridgeFeatureDisabled)
    }

    #[cfg(feature = "native-bridge")]
    pub fn estimate_presence(
        &mut self,
        frame: &VideoFrame,
    ) -> Result<Option<MediaPipePresencePoseEstimate>, MediaPipePresencePoseProviderError> {
        frame
            .validate()
            .map_err(|_| MediaPipePresencePoseProviderError::InvalidFrame)?;
        let request = MediaPipeBridgeFrameRequest {
            width: frame.width,
            height: frame.height,
            pixel_format: pixel_format_code(frame),
            _reserved: 0,
            data: frame.data.as_ptr(),
            data_len: frame.data.len(),
            face_box_x: 0.0,
            face_box_y: 0.0,
            face_box_width: 0.0,
            face_box_height: 0.0,
        };
        let mut result = MediaPipePresencePoseBridgeResult::default();
        let status =
            unsafe { (self.estimate_pose)(self.provider_handle.as_ptr(), &request, &mut result) };
        if status != 0 {
            return Err(MediaPipePresencePoseProviderError::InferenceFailed);
        }
        if result.detected == 0 {
            return Ok(None);
        }
        Ok(Some(MediaPipePresencePoseEstimate {
            confidence: result.confidence,
            bbox_center_x_ratio: result.bbox_center_x_ratio,
            bbox_area_ratio: result.bbox_area_ratio,
            normalized_x_min: result.normalized_x_min,
            normalized_y_min: result.normalized_y_min,
            normalized_x_max: result.normalized_x_max,
            normalized_y_max: result.normalized_y_max,
        }))
    }

    #[cfg(not(feature = "native-bridge"))]
    pub fn estimate_presence(
        &mut self,
        _frame: &VideoFrame,
    ) -> Result<Option<MediaPipePresencePoseEstimate>, MediaPipePresencePoseProviderError> {
        Err(MediaPipePresencePoseProviderError::NativeBridgeFeatureDisabled)
    }
}

#[cfg(feature = "native-bridge")]
impl Drop for MediaPipePresencePoseProvider {
    fn drop(&mut self) {
        unsafe {
            (self.destroy_provider)(self.provider_handle.as_ptr());
            FreeLibrary(self.library);
        }
    }
}

#[cfg(feature = "native-bridge")]
#[repr(C)]
#[derive(Clone, Copy)]
struct MediaPipePresencePoseBridgeOptions {
    running_mode: u32,
    min_landmark_visibility: f32,
    min_landmark_presence: f32,
}

#[cfg(feature = "native-bridge")]
#[repr(C)]
#[derive(Default)]
struct MediaPipePresencePoseBridgeResult {
    detected: u8,
    _reserved: [u8; 3],
    confidence: f32,
    bbox_center_x_ratio: f32,
    bbox_area_ratio: f32,
    normalized_x_min: f32,
    normalized_y_min: f32,
    normalized_x_max: f32,
    normalized_y_max: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_pose_config_uses_lite_visibility_defaults() {
        let config = MediaPipePresencePoseProviderConfig::new(
            PathBuf::from("bridge.dll"),
            PathBuf::from("pose_landmarker_lite.task"),
        );

        assert_eq!(config.min_landmark_visibility, 0.45);
        assert_eq!(config.min_landmark_presence, 0.45);
        assert_eq!(config.running_mode, MediaPipeRunningMode::Image);
    }

    #[cfg(not(feature = "native-bridge"))]
    #[test]
    fn default_build_reports_disabled_presence_pose_native_bridge() {
        let config = MediaPipePresencePoseProviderConfig::new(
            PathBuf::from("bridge.dll"),
            PathBuf::from("pose_landmarker_lite.task"),
        );

        assert!(matches!(
            MediaPipePresencePoseProvider::load(config),
            Err(MediaPipePresencePoseProviderError::NativeBridgeFeatureDisabled)
        ));
    }
}
