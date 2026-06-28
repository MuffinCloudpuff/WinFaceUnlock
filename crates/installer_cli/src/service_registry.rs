#![allow(unsafe_code)]

use std::{fmt, path::PathBuf};

const SERVICE_CONFIG_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\Service";

const REG_AUTH_MODE: &str = "AuthMode";
const REG_FACE_TEMPLATE_PATH: &str = "FaceTemplatePath";
const REG_CAMERA_ID: &str = "CameraId";
const REG_YUNET_MODEL_PATH: &str = "YuNetModelPath";
const REG_SFACE_MODEL_PATH: &str = "SFaceModelPath";
const REG_MINIFASNET_MODEL_PATH: &str = "MiniFasNetModelPath";
const REG_MINIFASNET_CROP_SCALE: &str = "MiniFasNetCropScale";
const REG_MINIFASNET_MIN_LIVE_SCORE: &str = "MiniFasNetMinLiveScore";
const REG_MINIFASNET_MIN_SPOOF_SCORE: &str = "MiniFasNetMinSpoofScore";
const REG_MINIFASNET_MAX_SPOOF_FRAME_RATIO: &str = "MiniFasNetMaxSpoofFrameRatio";
const REG_FRAME_WIDTH: &str = "FrameWidth";
const REG_FRAME_HEIGHT: &str = "FrameHeight";
const REG_MAX_AUTH_FRAMES: &str = "MaxAuthFrames";
const REG_REQUIRED_CONSECUTIVE: &str = "RequiredConsecutiveMatchCount";
const REG_MATCH_THRESHOLD: &str = "MatchThreshold";
const REG_PRESENCE_LOCK_ENABLED: &str = "PresenceLockEnabled";
const REG_PRESENCE_OWNER_MATCH_THRESHOLD: &str = "PresenceOwnerMatchThreshold";
const REG_PRESENCE_DETECTOR_KIND: &str = "PresenceDetectorKind";
const REG_PRESENCE_TRACKING_MODE: &str = "PresenceTrackingMode";
const REG_PRESENCE_DETECTOR_FPS: &str = "PresenceDetectorFps";
const REG_PRESENCE_UNLOAD_MODEL_WHEN_IDLE: &str = "PresenceUnloadModelWhenIdle";
const REG_PRESENCE_PERSON_CONFIDENCE_THRESHOLD: &str = "PresencePersonConfidenceThreshold";
const REG_PRESENCE_PERSON_DETECTOR_MODEL: &str = "PresencePersonDetectorModel";
const REG_PRESENCE_PERSON_SUSPECT_FPS: &str = "PresencePersonSuspectFps";
const REG_PRESENCE_ABSENT_REQUIRED_FRAMES: &str = "PresenceAbsentRequiredFrames";
const REG_PRESENCE_BOUNDARY_MARGIN_RATIO: &str = "PresenceBoundaryMarginRatio";
const REG_PRESENCE_MOVEMENT_DELTA_RATIO: &str = "PresenceMovementDeltaRatio";
const REG_PRESENCE_PERSON_MODEL_PATH: &str = "PresencePersonModelPath";
const REG_PRESENCE_PERSON_MODEL_CONFIG_PATH: &str = "PresencePersonModelConfigPath";
const REG_PRESENCE_PERSON_DEBUG_OUTPUT_DIR: &str = "PresencePersonDebugOutputDir";
const REG_PRESENCE_POSE_BRIDGE_DLL_PATH: &str = "PresencePoseBridgeDllPath";
const REG_PRESENCE_POSE_MODEL_PATH: &str = "PresencePoseModelPath";
const REG_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY: &str = "PresencePoseMinLandmarkVisibility";
const REG_PRESENCE_POSE_MIN_LANDMARK_PRESENCE: &str = "PresencePoseMinLandmarkPresence";

const AUTH_MODE_LOCAL_CAMERA: &str = "local-camera";
const DEFAULT_SERVICE_FACE_MATCH_THRESHOLD: f32 = 0.75;
const DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD: f32 = 0.50;
const DEFAULT_PRESENCE_DETECTOR_FPS: f32 = 2.0;
const DEFAULT_PRESENCE_PERSON_SUSPECT_FPS: f32 = 1.0;
const DEFAULT_PRESENCE_PERSON_CONFIDENCE_THRESHOLD: f32 = 0.50;
const DEFAULT_PRESENCE_ABSENT_REQUIRED_FRAMES: u32 = 3;
const DEFAULT_PRESENCE_BOUNDARY_MARGIN_RATIO: f32 = 0.12;
const DEFAULT_PRESENCE_MOVEMENT_DELTA_RATIO: f32 = 0.04;
const DEFAULT_PRESENCE_DETECTOR_KIND: &str = "opencv-dnn-person";
const DEFAULT_PRESENCE_TRACKING_MODE: &str = "continuous-low-fps";
const DEFAULT_PRESENCE_PERSON_DETECTOR_MODEL: &str = "ort-yolov8-onnx";
const DEFAULT_PRESENCE_PERSON_MODEL_PATH: &str = r"models\yolov8n.onnx";
const DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH: &str = r"native\winfaceunlock_mediapipe_bridge.dll";
const DEFAULT_PRESENCE_POSE_MODEL_PATH: &str = r"models\pose_landmarker_lite.task";
const DEFAULT_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY: f32 = 0.45;
const DEFAULT_PRESENCE_POSE_MIN_LANDMARK_PRESENCE: f32 = 0.45;
const DEFAULT_MINIFASNET_MODEL_PATH: &str = r"models\minifasnet_v2.onnx";
const DEFAULT_MINIFASNET_CROP_SCALE: f32 = 2.7;
const DEFAULT_MINIFASNET_MIN_LIVE_SCORE: f32 = 0.80;
const DEFAULT_MINIFASNET_MIN_SPOOF_SCORE: f32 = 0.70;
const DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO: f32 = 0.40;

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceAuthRegistryConfig {
    pub auth_mode: String,
    pub face_template_path: PathBuf,
    pub camera_id: String,
    pub yunet_model_path: PathBuf,
    pub sface_model_path: PathBuf,
    pub minifasnet_model_path: PathBuf,
    pub minifasnet_crop_scale: f32,
    pub minifasnet_min_live_score: f32,
    pub minifasnet_min_spoof_score: f32,
    pub minifasnet_max_spoof_frame_ratio: f32,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub max_auth_frames: u32,
    pub required_consecutive_match_count: u32,
    pub match_threshold: f32,
    pub presence_lock_enabled: bool,
    pub presence_owner_match_threshold: f32,
    pub presence_detector_kind: String,
    pub presence_tracking_mode: String,
    pub presence_detector_fps: f32,
    pub presence_unload_model_when_idle: bool,
    pub presence_person_confidence_threshold: f32,
    pub presence_person_detector_model: String,
    pub presence_person_suspect_fps: f32,
    pub presence_absent_required_frames: u32,
    pub presence_boundary_margin_ratio: f32,
    pub presence_movement_delta_ratio: f32,
    pub presence_person_model_path: PathBuf,
    pub presence_person_model_config_path: Option<PathBuf>,
    pub presence_pose_bridge_dll_path: PathBuf,
    pub presence_pose_model_path: PathBuf,
    pub presence_pose_min_landmark_visibility: f32,
    pub presence_pose_min_landmark_presence: f32,
}

impl ServiceAuthRegistryConfig {
    pub fn local_camera(
        face_template_path: PathBuf,
        yunet_model_path: PathBuf,
        sface_model_path: PathBuf,
    ) -> Self {
        Self {
            auth_mode: AUTH_MODE_LOCAL_CAMERA.to_owned(),
            face_template_path,
            camera_id: "opencv-index:0".to_owned(),
            yunet_model_path,
            sface_model_path,
            minifasnet_model_path: PathBuf::from(DEFAULT_MINIFASNET_MODEL_PATH),
            minifasnet_crop_scale: DEFAULT_MINIFASNET_CROP_SCALE,
            minifasnet_min_live_score: DEFAULT_MINIFASNET_MIN_LIVE_SCORE,
            minifasnet_min_spoof_score: DEFAULT_MINIFASNET_MIN_SPOOF_SCORE,
            minifasnet_max_spoof_frame_ratio: DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO,
            frame_width: None,
            frame_height: None,
            max_auth_frames: 30,
            required_consecutive_match_count: 2,
            match_threshold: DEFAULT_SERVICE_FACE_MATCH_THRESHOLD,
            presence_lock_enabled: false,
            presence_owner_match_threshold: DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD,
            presence_detector_kind: DEFAULT_PRESENCE_DETECTOR_KIND.to_owned(),
            presence_tracking_mode: DEFAULT_PRESENCE_TRACKING_MODE.to_owned(),
            presence_detector_fps: DEFAULT_PRESENCE_DETECTOR_FPS,
            presence_unload_model_when_idle: false,
            presence_person_confidence_threshold: DEFAULT_PRESENCE_PERSON_CONFIDENCE_THRESHOLD,
            presence_person_detector_model: DEFAULT_PRESENCE_PERSON_DETECTOR_MODEL.to_owned(),
            presence_person_suspect_fps: DEFAULT_PRESENCE_PERSON_SUSPECT_FPS,
            presence_absent_required_frames: DEFAULT_PRESENCE_ABSENT_REQUIRED_FRAMES,
            presence_boundary_margin_ratio: DEFAULT_PRESENCE_BOUNDARY_MARGIN_RATIO,
            presence_movement_delta_ratio: DEFAULT_PRESENCE_MOVEMENT_DELTA_RATIO,
            presence_person_model_path: PathBuf::from(DEFAULT_PRESENCE_PERSON_MODEL_PATH),
            presence_person_model_config_path: None,
            presence_pose_bridge_dll_path: PathBuf::from(DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH),
            presence_pose_model_path: PathBuf::from(DEFAULT_PRESENCE_POSE_MODEL_PATH),
            presence_pose_min_landmark_visibility: DEFAULT_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY,
            presence_pose_min_landmark_presence: DEFAULT_PRESENCE_POSE_MIN_LANDMARK_PRESENCE,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceAuthRegistryStatus {
    pub registry_config_exists: bool,
    pub auth_mode: Option<String>,
    pub face_template_path: Option<String>,
    pub camera_id: Option<String>,
    pub match_threshold: Option<String>,
    pub minifasnet_model_path: Option<String>,
    pub minifasnet_max_spoof_frame_ratio: Option<String>,
    pub presence_lock_enabled: Option<String>,
    pub presence_owner_match_threshold: Option<String>,
    pub presence_detector_kind: Option<String>,
    pub presence_tracking_mode: Option<String>,
    pub presence_detector_fps: Option<String>,
    pub presence_unload_model_when_idle: Option<String>,
    pub presence_person_confidence_threshold: Option<String>,
    pub presence_person_detector_model: Option<String>,
    pub presence_person_suspect_fps: Option<String>,
    pub presence_absent_required_frames: Option<String>,
    pub presence_boundary_margin_ratio: Option<String>,
    pub presence_movement_delta_ratio: Option<String>,
    pub presence_person_model_path: Option<String>,
    pub presence_person_model_config_path: Option<String>,
    pub presence_person_debug_output_dir: Option<String>,
    pub presence_pose_bridge_dll_path: Option<String>,
    pub presence_pose_model_path: Option<String>,
    pub presence_pose_min_landmark_visibility: Option<String>,
    pub presence_pose_min_landmark_presence: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServicePresenceRegistryPatch {
    pub presence_lock_enabled: Option<bool>,
    pub presence_owner_match_threshold: Option<f32>,
    pub presence_detector_kind: Option<String>,
    pub presence_tracking_mode: Option<String>,
    pub presence_detector_fps: Option<f32>,
    pub presence_unload_model_when_idle: Option<bool>,
    pub presence_person_confidence_threshold: Option<f32>,
    pub presence_person_detector_model: Option<String>,
    pub presence_person_suspect_fps: Option<f32>,
    pub presence_absent_required_frames: Option<u32>,
    pub presence_boundary_margin_ratio: Option<f32>,
    pub presence_movement_delta_ratio: Option<f32>,
    pub presence_person_model_path: Option<PathBuf>,
    pub presence_person_model_config_path: Option<Option<PathBuf>>,
    pub presence_person_debug_output_dir: Option<Option<PathBuf>>,
    pub presence_pose_bridge_dll_path: Option<PathBuf>,
    pub presence_pose_model_path: Option<PathBuf>,
    pub presence_pose_min_landmark_visibility: Option<f32>,
    pub presence_pose_min_landmark_presence: Option<f32>,
}

pub struct ServiceAuthRegistry;

impl ServiceAuthRegistry {
    pub fn configure_local_camera(
        config: &ServiceAuthRegistryConfig,
    ) -> Result<(), ServiceRegistryError> {
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_AUTH_MODE,
            &config.auth_mode,
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_FACE_TEMPLATE_PATH,
            &config.face_template_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_CAMERA_ID,
            &config.camera_id,
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_YUNET_MODEL_PATH,
            &config.yunet_model_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_SFACE_MODEL_PATH,
            &config.sface_model_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MINIFASNET_MODEL_PATH,
            &config.minifasnet_model_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MINIFASNET_CROP_SCALE,
            &config.minifasnet_crop_scale.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MINIFASNET_MIN_LIVE_SCORE,
            &config.minifasnet_min_live_score.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MINIFASNET_MIN_SPOOF_SCORE,
            &config.minifasnet_min_spoof_score.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MINIFASNET_MAX_SPOOF_FRAME_RATIO,
            &config.minifasnet_max_spoof_frame_ratio.to_string(),
        )?;
        set_optional_u32(REG_FRAME_WIDTH, config.frame_width)?;
        set_optional_u32(REG_FRAME_HEIGHT, config.frame_height)?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MAX_AUTH_FRAMES,
            &config.max_auth_frames.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_REQUIRED_CONSECUTIVE,
            &config.required_consecutive_match_count.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_MATCH_THRESHOLD,
            &config.match_threshold.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_LOCK_ENABLED,
            bool_registry_value(config.presence_lock_enabled),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_OWNER_MATCH_THRESHOLD,
            &config.presence_owner_match_threshold.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_DETECTOR_KIND,
            &config.presence_detector_kind,
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_TRACKING_MODE,
            &config.presence_tracking_mode,
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_DETECTOR_FPS,
            &config.presence_detector_fps.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_UNLOAD_MODEL_WHEN_IDLE,
            bool_registry_value(config.presence_unload_model_when_idle),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_PERSON_CONFIDENCE_THRESHOLD,
            &config.presence_person_confidence_threshold.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_PERSON_DETECTOR_MODEL,
            &config.presence_person_detector_model,
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_PERSON_SUSPECT_FPS,
            &config.presence_person_suspect_fps.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_ABSENT_REQUIRED_FRAMES,
            &config.presence_absent_required_frames.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_BOUNDARY_MARGIN_RATIO,
            &config.presence_boundary_margin_ratio.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_MOVEMENT_DELTA_RATIO,
            &config.presence_movement_delta_ratio.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_PERSON_MODEL_PATH,
            &config.presence_person_model_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_PERSON_MODEL_CONFIG_PATH,
            &config
                .presence_person_model_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_POSE_BRIDGE_DLL_PATH,
            &config.presence_pose_bridge_dll_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_POSE_MODEL_PATH,
            &config.presence_pose_model_path.display().to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY,
            &config.presence_pose_min_landmark_visibility.to_string(),
        )?;
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            REG_PRESENCE_POSE_MIN_LANDMARK_PRESENCE,
            &config.presence_pose_min_landmark_presence.to_string(),
        )?;
        Ok(())
    }

    pub fn configure_presence_lock(
        patch: &ServicePresenceRegistryPatch,
    ) -> Result<(), ServiceRegistryError> {
        if let Some(value) = patch.presence_lock_enabled {
            registry::set_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_LOCK_ENABLED,
                bool_registry_value(value),
            )?;
        }
        set_optional_f32(
            REG_PRESENCE_OWNER_MATCH_THRESHOLD,
            patch.presence_owner_match_threshold,
        )?;
        set_optional_string(
            REG_PRESENCE_DETECTOR_KIND,
            patch.presence_detector_kind.as_deref(),
        )?;
        set_optional_string(
            REG_PRESENCE_TRACKING_MODE,
            patch.presence_tracking_mode.as_deref(),
        )?;
        set_optional_f32(REG_PRESENCE_DETECTOR_FPS, patch.presence_detector_fps)?;
        if let Some(value) = patch.presence_unload_model_when_idle {
            registry::set_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_UNLOAD_MODEL_WHEN_IDLE,
                bool_registry_value(value),
            )?;
        }
        set_optional_f32(
            REG_PRESENCE_PERSON_CONFIDENCE_THRESHOLD,
            patch.presence_person_confidence_threshold,
        )?;
        set_optional_string(
            REG_PRESENCE_PERSON_DETECTOR_MODEL,
            patch.presence_person_detector_model.as_deref(),
        )?;
        set_optional_f32(
            REG_PRESENCE_PERSON_SUSPECT_FPS,
            patch.presence_person_suspect_fps,
        )?;
        set_optional_u32(
            REG_PRESENCE_ABSENT_REQUIRED_FRAMES,
            patch.presence_absent_required_frames,
        )?;
        set_optional_f32(
            REG_PRESENCE_BOUNDARY_MARGIN_RATIO,
            patch.presence_boundary_margin_ratio,
        )?;
        set_optional_f32(
            REG_PRESENCE_MOVEMENT_DELTA_RATIO,
            patch.presence_movement_delta_ratio,
        )?;
        set_optional_path(
            REG_PRESENCE_PERSON_MODEL_PATH,
            &patch.presence_person_model_path,
        )?;
        set_optional_nullable_path(
            REG_PRESENCE_PERSON_MODEL_CONFIG_PATH,
            &patch.presence_person_model_config_path,
        )?;
        set_optional_nullable_path(
            REG_PRESENCE_PERSON_DEBUG_OUTPUT_DIR,
            &patch.presence_person_debug_output_dir,
        )?;
        set_optional_path(
            REG_PRESENCE_POSE_BRIDGE_DLL_PATH,
            &patch.presence_pose_bridge_dll_path,
        )?;
        set_optional_path(
            REG_PRESENCE_POSE_MODEL_PATH,
            &patch.presence_pose_model_path,
        )?;
        set_optional_f32(
            REG_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY,
            patch.presence_pose_min_landmark_visibility,
        )?;
        set_optional_f32(
            REG_PRESENCE_POSE_MIN_LANDMARK_PRESENCE,
            patch.presence_pose_min_landmark_presence,
        )?;
        Ok(())
    }

    pub fn status() -> ServiceAuthRegistryStatus {
        ServiceAuthRegistryStatus {
            registry_config_exists: registry::key_exists(SERVICE_CONFIG_REGISTRY_PATH),
            auth_mode: registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_AUTH_MODE),
            face_template_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_FACE_TEMPLATE_PATH,
            ),
            camera_id: registry::read_string_value(SERVICE_CONFIG_REGISTRY_PATH, REG_CAMERA_ID),
            match_threshold: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_MATCH_THRESHOLD,
            ),
            minifasnet_model_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_MINIFASNET_MODEL_PATH,
            ),
            minifasnet_max_spoof_frame_ratio: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_MINIFASNET_MAX_SPOOF_FRAME_RATIO,
            ),
            presence_lock_enabled: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_LOCK_ENABLED,
            ),
            presence_owner_match_threshold: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_OWNER_MATCH_THRESHOLD,
            ),
            presence_detector_kind: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_DETECTOR_KIND,
            ),
            presence_tracking_mode: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_TRACKING_MODE,
            ),
            presence_detector_fps: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_DETECTOR_FPS,
            ),
            presence_unload_model_when_idle: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_UNLOAD_MODEL_WHEN_IDLE,
            ),
            presence_person_confidence_threshold: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_CONFIDENCE_THRESHOLD,
            ),
            presence_person_detector_model: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_DETECTOR_MODEL,
            ),
            presence_person_suspect_fps: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_SUSPECT_FPS,
            ),
            presence_absent_required_frames: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_ABSENT_REQUIRED_FRAMES,
            ),
            presence_boundary_margin_ratio: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_BOUNDARY_MARGIN_RATIO,
            ),
            presence_movement_delta_ratio: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_MOVEMENT_DELTA_RATIO,
            ),
            presence_person_model_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_MODEL_PATH,
            ),
            presence_person_model_config_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_MODEL_CONFIG_PATH,
            ),
            presence_person_debug_output_dir: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_PERSON_DEBUG_OUTPUT_DIR,
            ),
            presence_pose_bridge_dll_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_POSE_BRIDGE_DLL_PATH,
            ),
            presence_pose_model_path: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_POSE_MODEL_PATH,
            ),
            presence_pose_min_landmark_visibility: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY,
            ),
            presence_pose_min_landmark_presence: registry::read_string_value(
                SERVICE_CONFIG_REGISTRY_PATH,
                REG_PRESENCE_POSE_MIN_LANDMARK_PRESENCE,
            ),
        }
    }
}

fn bool_registry_value(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn set_optional_u32(
    value_name: &'static str,
    value: Option<u32>,
) -> Result<(), ServiceRegistryError> {
    if let Some(value) = value {
        registry::set_string_value(SERVICE_CONFIG_REGISTRY_PATH, value_name, &value.to_string())?;
    }
    Ok(())
}

fn set_optional_f32(
    value_name: &'static str,
    value: Option<f32>,
) -> Result<(), ServiceRegistryError> {
    if let Some(value) = value {
        registry::set_string_value(SERVICE_CONFIG_REGISTRY_PATH, value_name, &value.to_string())?;
    }
    Ok(())
}

fn set_optional_string(
    value_name: &'static str,
    value: Option<&str>,
) -> Result<(), ServiceRegistryError> {
    if let Some(value) = value {
        registry::set_string_value(SERVICE_CONFIG_REGISTRY_PATH, value_name, value)?;
    }
    Ok(())
}

fn set_optional_path(
    value_name: &'static str,
    value: &Option<PathBuf>,
) -> Result<(), ServiceRegistryError> {
    if let Some(value) = value {
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            value_name,
            &value.display().to_string(),
        )?;
    }
    Ok(())
}

fn set_optional_nullable_path(
    value_name: &'static str,
    value: &Option<Option<PathBuf>>,
) -> Result<(), ServiceRegistryError> {
    if let Some(value) = value {
        registry::set_string_value(
            SERVICE_CONFIG_REGISTRY_PATH,
            value_name,
            &value
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        )?;
    }
    Ok(())
}

#[derive(Debug)]
pub enum ServiceRegistryError {
    WindowsRegistry {
        operation: &'static str,
        path: String,
        code: u32,
    },
}

impl fmt::Display for ServiceRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowsRegistry {
                operation,
                path,
                code,
            } => write!(
                formatter,
                "windows registry {operation} failed for {path}: error {code}"
            ),
        }
    }
}

impl std::error::Error for ServiceRegistryError {}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
            RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        },
    };

    use super::ServiceRegistryError;

    pub fn set_string_value(
        path: &str,
        value_name: &str,
        value: &str,
    ) -> Result<(), ServiceRegistryError> {
        let key = create_key(path)?;
        let name = to_wide_null(value_name);
        let mut data = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let status = unsafe {
            RegSetValueExW(
                key.raw,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_mut_ptr().cast::<u8>(),
                (data.len() * size_of::<u16>()) as u32,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("set value", path, status));
        }
        Ok(())
    }

    pub fn read_string_value(path: &str, value_name: &str) -> Option<String> {
        let key = open_key(path)?;
        let name = to_wide_null(value_name);
        let mut data_type = 0_u32;
        let mut byte_len = 0_u32;
        let probe_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                ptr::null_mut(),
                &mut byte_len,
            )
        };
        if probe_status != ERROR_SUCCESS || data_type != REG_SZ || byte_len < 2 {
            return None;
        }

        let mut data = vec![0_u16; byte_len as usize / size_of::<u16>()];
        let query_status = unsafe {
            RegQueryValueExW(
                key.raw,
                name.as_ptr(),
                ptr::null_mut(),
                &mut data_type,
                data.as_mut_ptr().cast::<u8>(),
                &mut byte_len,
            )
        };
        if query_status != ERROR_SUCCESS || data_type != REG_SZ {
            return None;
        }
        if let Some(terminator_index) = data.iter().position(|value| *value == 0) {
            data.truncate(terminator_index);
        }
        String::from_utf16(&data).ok()
    }

    pub fn key_exists(path: &str) -> bool {
        open_key(path).is_some()
    }

    fn create_key(path: &str) -> Result<OwnedRegistryKey, ServiceRegistryError> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                ptr::null_mut(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(registry_error("create key", path, status));
        }
        Ok(OwnedRegistryKey { raw: key })
    }

    fn open_key(path: &str) -> Option<OwnedRegistryKey> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if status == ERROR_SUCCESS {
            Some(OwnedRegistryKey { raw: key })
        } else {
            None
        }
    }

    struct OwnedRegistryKey {
        raw: HKEY,
    }

    impl Drop for OwnedRegistryKey {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    let _ = RegCloseKey(self.raw);
                }
            }
        }
    }

    fn registry_error(
        operation: &'static str,
        path: &str,
        code: WIN32_ERROR,
    ) -> ServiceRegistryError {
        ServiceRegistryError::WindowsRegistry {
            operation,
            path: path.to_owned(),
            code,
        }
    }

    fn to_wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod registry {
    use super::ServiceRegistryError;

    pub fn set_string_value(
        path: &str,
        _value_name: &str,
        _value: &str,
    ) -> Result<(), ServiceRegistryError> {
        Err(unsupported(path, "set value"))
    }

    pub fn read_string_value(_path: &str, _value_name: &str) -> Option<String> {
        None
    }

    pub fn key_exists(_path: &str) -> bool {
        false
    }

    fn unsupported(path: &str, operation: &'static str) -> ServiceRegistryError {
        ServiceRegistryError::WindowsRegistry {
            operation,
            path: path.to_owned(),
            code: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_camera_config_uses_project_defaults() {
        let config = ServiceAuthRegistryConfig::local_camera(
            PathBuf::from(r"D:\WinFaceUnlock\face.json"),
            PathBuf::from(r"D:\WinFaceUnlock\yunet.onnx"),
            PathBuf::from(r"D:\WinFaceUnlock\sface.onnx"),
        );

        assert_eq!(config.auth_mode, "local-camera");
        assert_eq!(config.camera_id, "opencv-index:0");
        assert_eq!(config.required_consecutive_match_count, 2);
        assert_eq!(config.match_threshold, DEFAULT_SERVICE_FACE_MATCH_THRESHOLD);
        assert!(!config.presence_lock_enabled);
        assert_eq!(
            config.presence_owner_match_threshold,
            DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD
        );
        assert_eq!(
            config.presence_detector_kind,
            DEFAULT_PRESENCE_DETECTOR_KIND
        );
        assert_eq!(
            config.presence_tracking_mode,
            DEFAULT_PRESENCE_TRACKING_MODE
        );
        assert_eq!(config.presence_detector_fps, DEFAULT_PRESENCE_DETECTOR_FPS);
        assert!(!config.presence_unload_model_when_idle);
        assert_eq!(
            config.presence_person_detector_model,
            DEFAULT_PRESENCE_PERSON_DETECTOR_MODEL
        );
        assert_eq!(
            config.presence_person_model_path,
            PathBuf::from(DEFAULT_PRESENCE_PERSON_MODEL_PATH)
        );
        assert_eq!(config.presence_person_model_config_path, None);
        assert_eq!(
            config.presence_pose_bridge_dll_path,
            PathBuf::from(DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH)
        );
        assert_eq!(
            config.presence_pose_model_path,
            PathBuf::from(DEFAULT_PRESENCE_POSE_MODEL_PATH)
        );
        assert_eq!(
            config.presence_pose_min_landmark_visibility,
            DEFAULT_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY
        );
        assert_eq!(
            config.presence_pose_min_landmark_presence,
            DEFAULT_PRESENCE_POSE_MIN_LANDMARK_PRESENCE
        );
        assert_eq!(
            config.presence_person_suspect_fps,
            DEFAULT_PRESENCE_PERSON_SUSPECT_FPS
        );
        assert_eq!(
            config.presence_person_confidence_threshold,
            DEFAULT_PRESENCE_PERSON_CONFIDENCE_THRESHOLD
        );
        assert_eq!(
            config.presence_absent_required_frames,
            DEFAULT_PRESENCE_ABSENT_REQUIRED_FRAMES
        );
        assert_eq!(
            config.minifasnet_model_path,
            PathBuf::from(DEFAULT_MINIFASNET_MODEL_PATH)
        );
        assert_eq!(config.minifasnet_crop_scale, DEFAULT_MINIFASNET_CROP_SCALE);
        assert_eq!(
            config.minifasnet_max_spoof_frame_ratio,
            DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO
        );
    }
}
