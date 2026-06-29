use std::path::{Path, PathBuf};

use common_protocol::ProtocolError;
use face_liveness::MiniFasNetLivenessProviderConfig;
use video_provider::{CameraId, OpenCvCameraProviderConfig};

const ENV_AUTH_MODE: &str = "WINFACEUNLOCK_AUTH_MODE";
const ENV_FACE_TEMPLATE_PATH: &str = "WINFACEUNLOCK_FACE_TEMPLATE_PATH";
const ENV_CAMERA_ID: &str = "WINFACEUNLOCK_CAMERA_ID";
const ENV_YUNET_MODEL_PATH: &str = "WINFACEUNLOCK_YUNET_MODEL_PATH";
const ENV_SFACE_MODEL_PATH: &str = "WINFACEUNLOCK_SFACE_MODEL_PATH";
const ENV_MINIFASNET_MODEL_PATH: &str = "WINFACEUNLOCK_MINIFASNET_MODEL_PATH";
const ENV_MINIFASNET_CROP_SCALE: &str = "WINFACEUNLOCK_MINIFASNET_CROP_SCALE";
const ENV_MINIFASNET_MIN_LIVE_SCORE: &str = "WINFACEUNLOCK_MINIFASNET_MIN_LIVE_SCORE";
const ENV_MINIFASNET_MIN_SPOOF_SCORE: &str = "WINFACEUNLOCK_MINIFASNET_MIN_SPOOF_SCORE";
const ENV_MINIFASNET_MAX_SPOOF_FRAME_RATIO: &str = "WINFACEUNLOCK_MINIFASNET_MAX_SPOOF_FRAME_RATIO";
const ENV_FRAME_WIDTH: &str = "WINFACEUNLOCK_FRAME_WIDTH";
const ENV_FRAME_HEIGHT: &str = "WINFACEUNLOCK_FRAME_HEIGHT";
const ENV_MAX_AUTH_FRAMES: &str = "WINFACEUNLOCK_MAX_AUTH_FRAMES";
const ENV_REQUIRED_CONSECUTIVE: &str = "WINFACEUNLOCK_REQUIRED_CONSECUTIVE";
const ENV_MATCH_THRESHOLD: &str = "WINFACEUNLOCK_MATCH_THRESHOLD";
const ENV_PRESENCE_LOCK_ENABLED: &str = "WINFACEUNLOCK_PRESENCE_LOCK_ENABLED";
const ENV_PRESENCE_OWNER_MATCH_THRESHOLD: &str = "WINFACEUNLOCK_PRESENCE_OWNER_MATCH_THRESHOLD";
const ENV_PRESENCE_DETECTOR_KIND: &str = "WINFACEUNLOCK_PRESENCE_DETECTOR_KIND";
const ENV_PRESENCE_TRACKING_MODE: &str = "WINFACEUNLOCK_PRESENCE_TRACKING_MODE";
const ENV_PRESENCE_DETECTOR_FPS: &str = "WINFACEUNLOCK_PRESENCE_DETECTOR_FPS";
const ENV_PRESENCE_UNLOAD_MODEL_WHEN_IDLE: &str = "WINFACEUNLOCK_PRESENCE_UNLOAD_MODEL_WHEN_IDLE";
const ENV_PRESENCE_PERSON_CONFIDENCE_THRESHOLD: &str =
    "WINFACEUNLOCK_PRESENCE_PERSON_CONFIDENCE_THRESHOLD";
const ENV_PRESENCE_PERSON_DETECTOR_MODEL: &str = "WINFACEUNLOCK_PRESENCE_PERSON_DETECTOR_MODEL";
const ENV_PRESENCE_PERSON_SUSPECT_FPS: &str = "WINFACEUNLOCK_PRESENCE_PERSON_SUSPECT_FPS";
const ENV_PRESENCE_ABSENT_REQUIRED_FRAMES: &str = "WINFACEUNLOCK_PRESENCE_ABSENT_REQUIRED_FRAMES";
const ENV_PRESENCE_BOUNDARY_MARGIN_RATIO: &str = "WINFACEUNLOCK_PRESENCE_BOUNDARY_MARGIN_RATIO";
const ENV_PRESENCE_MOVEMENT_DELTA_RATIO: &str = "WINFACEUNLOCK_PRESENCE_MOVEMENT_DELTA_RATIO";
const ENV_PRESENCE_PERSON_MODEL_PATH: &str = "WINFACEUNLOCK_PRESENCE_PERSON_MODEL_PATH";
const ENV_PRESENCE_PERSON_MODEL_CONFIG_PATH: &str =
    "WINFACEUNLOCK_PRESENCE_PERSON_MODEL_CONFIG_PATH";
const ENV_PRESENCE_PERSON_DEBUG_OUTPUT_DIR: &str = "WINFACEUNLOCK_PRESENCE_PERSON_DEBUG_OUTPUT_DIR";
const ENV_PRESENCE_POSE_BRIDGE_DLL_PATH: &str = "WINFACEUNLOCK_PRESENCE_POSE_BRIDGE_DLL_PATH";
const ENV_PRESENCE_POSE_MODEL_PATH: &str = "WINFACEUNLOCK_PRESENCE_POSE_MODEL_PATH";
const ENV_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY: &str =
    "WINFACEUNLOCK_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY";
const ENV_PRESENCE_POSE_MIN_LANDMARK_PRESENCE: &str =
    "WINFACEUNLOCK_PRESENCE_POSE_MIN_LANDMARK_PRESENCE";
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

const AUTH_MODE_MANUAL_TEST: &str = "manual-test";
const AUTH_MODE_LOCAL_CAMERA: &str = "local-camera";
const DEFAULT_YUNET_MODEL_PATH: &str = "models/face_detection_yunet_2023mar.onnx";
const DEFAULT_SFACE_MODEL_PATH: &str = "models/ghostfacenet_v1_stride2.onnx";
const DEFAULT_MINIFASNET_MODEL_PATH: &str = "models/minifasnet_v2.onnx";
const DEFAULT_MINIFASNET_CROP_SCALE: f32 = 2.7;
const DEFAULT_MINIFASNET_MIN_LIVE_SCORE: f32 = 0.80;
const DEFAULT_MINIFASNET_MIN_SPOOF_SCORE: f32 = 0.70;
const DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO: f32 = 0.40;
const DEFAULT_MAX_AUTH_FRAMES: u32 = 30;
const DEFAULT_REQUIRED_CONSECUTIVE_MATCH_COUNT: u32 = 2;
const DEFAULT_MAX_CAMERA_INDEX: u32 = 8;
const DEFAULT_SERVICE_FACE_MATCH_THRESHOLD: f32 = 0.75;
const DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD: f32 = 0.50;
const DEFAULT_PRESENCE_DETECTOR_FPS: f32 = 2.0;
const DEFAULT_PRESENCE_PERSON_SUSPECT_FPS: f32 = 1.0;
const DEFAULT_PRESENCE_PERSON_CONFIDENCE_THRESHOLD: f32 = 0.50;
const DEFAULT_PRESENCE_ABSENT_REQUIRED_FRAMES: u32 = 3;
const DEFAULT_PRESENCE_BOUNDARY_MARGIN_RATIO: f32 = 0.12;
const DEFAULT_PRESENCE_MOVEMENT_DELTA_RATIO: f32 = 0.04;
const DEFAULT_PRESENCE_YOLOV8_PERSON_MODEL_PATH: &str = "models/yolov8n.onnx";
const DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH: &str = "native/winfaceunlock_mediapipe_bridge.dll";
const DEFAULT_PRESENCE_POSE_MODEL_PATH: &str = "models/pose_landmarker_lite.task";
const DEFAULT_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY: f32 = 0.45;
const DEFAULT_PRESENCE_POSE_MIN_LANDMARK_PRESENCE: f32 = 0.45;
const PRESENCE_DETECTOR_KIND_FACE_OWNER_MATCH: &str = "face-owner-match";
const PRESENCE_DETECTOR_KIND_OPENCV_DNN_PERSON: &str = "opencv-dnn-person";
const PRESENCE_DETECTOR_KIND_MEDIAPIPE_POSE_LITE: &str = "mediapipe-pose-lite";
const PRESENCE_TRACKING_MODE_FACE_POLICY: &str = "face-policy";
const PRESENCE_TRACKING_MODE_CONTINUOUS_LOW_FPS: &str = "continuous-low-fps";
const PRESENCE_PERSON_DETECTOR_MODEL_MOBILENET_SSD: &str = "mobilenet-ssd";
const PRESENCE_PERSON_DETECTOR_MODEL_YOLOV8_ONNX: &str = "yolov8-onnx";
const PRESENCE_PERSON_DETECTOR_MODEL_ORT_YOLOV8_ONNX: &str = "ort-yolov8-onnx";

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceAuthConfig {
    pub auth_mode: ServiceAuthMode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServiceAuthMode {
    ManualTestOnly,
    LocalCamera(Box<LocalCameraAuthConfig>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalCameraAuthConfig {
    pub face_template_path: PathBuf,
    pub camera_id: CameraId,
    pub camera_config: OpenCvCameraProviderConfig,
    pub yunet_model_path: PathBuf,
    pub sface_model_path: PathBuf,
    pub minifasnet_config: MiniFasNetLivenessProviderConfig,
    pub minifasnet_max_spoof_frame_ratio: f32,
    pub max_auth_frames: u32,
    pub required_consecutive_match_count: u32,
    pub match_threshold: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServicePresenceConfig {
    pub presence_lock_enabled: bool,
    pub presence_owner_match_threshold: f32,
    pub presence_detector_kind: PresenceDetectorKind,
    pub presence_tracking_mode: PresenceTrackingMode,
    pub presence_detector_fps: f32,
    pub presence_unload_model_when_idle: bool,
    pub presence_person_confidence_threshold: f32,
    pub presence_person_detector_model: PresencePersonDetectorModel,
    pub presence_person_suspect_fps: f32,
    pub presence_absent_required_frames: u32,
    pub presence_boundary_margin_ratio: f32,
    pub presence_movement_delta_ratio: f32,
    pub presence_person_model_path: PathBuf,
    pub presence_person_model_config_path: Option<PathBuf>,
    pub presence_person_debug_output_dir: Option<PathBuf>,
    pub presence_pose_bridge_dll_path: PathBuf,
    pub presence_pose_model_path: PathBuf,
    pub presence_pose_min_landmark_visibility: f32,
    pub presence_pose_min_landmark_presence: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceDetectorKind {
    FaceOwnerMatch,
    OpenCvDnnPerson,
    MediaPipePoseLite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresenceTrackingMode {
    FacePolicy,
    ContinuousLowFps,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresencePersonDetectorModel {
    MobileNetSsd,
    YoloV8Onnx,
    OrtYoloV8Onnx,
}

impl ServicePresenceConfig {
    pub fn from_environment() -> Result<Self, ProtocolError> {
        let mut config = Self::from_lookup(|env_name| {
            std::env::var(env_name)
                .ok()
                .or_else(|| registry_value_for_env_name(env_name))
        })?;
        if let Some(install_dir) = current_exe_dir() {
            config.resolve_relative_paths_against_install_dir(&install_dir);
        }
        Ok(config)
    }

    fn from_lookup(
        mut lookup: impl FnMut(&'static str) -> Option<String>,
    ) -> Result<Self, ProtocolError> {
        let presence_person_detector_model = optional_presence_person_detector_model(
            &mut lookup,
            ENV_PRESENCE_PERSON_DETECTOR_MODEL,
        )?
        .unwrap_or(PresencePersonDetectorModel::OrtYoloV8Onnx);
        let default_person_model_path = match presence_person_detector_model {
            PresencePersonDetectorModel::MobileNetSsd => "models/MobileNetSSD_deploy.caffemodel",
            PresencePersonDetectorModel::YoloV8Onnx
            | PresencePersonDetectorModel::OrtYoloV8Onnx => {
                DEFAULT_PRESENCE_YOLOV8_PERSON_MODEL_PATH
            }
        };
        let default_person_model_config_path = match presence_person_detector_model {
            PresencePersonDetectorModel::MobileNetSsd => {
                Some(PathBuf::from("models/MobileNetSSD_deploy.prototxt"))
            }
            PresencePersonDetectorModel::YoloV8Onnx
            | PresencePersonDetectorModel::OrtYoloV8Onnx => None,
        };

        Ok(Self {
            presence_lock_enabled: optional_bool(&mut lookup, ENV_PRESENCE_LOCK_ENABLED)?
                .unwrap_or(false),
            presence_owner_match_threshold: optional_f32(
                &mut lookup,
                ENV_PRESENCE_OWNER_MATCH_THRESHOLD,
            )?
            .unwrap_or(DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD),
            presence_detector_kind: optional_presence_detector_kind(
                &mut lookup,
                ENV_PRESENCE_DETECTOR_KIND,
            )?
            .unwrap_or(PresenceDetectorKind::OpenCvDnnPerson),
            presence_tracking_mode: optional_presence_tracking_mode(
                &mut lookup,
                ENV_PRESENCE_TRACKING_MODE,
            )?
            .unwrap_or(PresenceTrackingMode::ContinuousLowFps),
            presence_detector_fps: optional_f32(&mut lookup, ENV_PRESENCE_DETECTOR_FPS)?
                .unwrap_or(DEFAULT_PRESENCE_DETECTOR_FPS),
            presence_unload_model_when_idle: optional_bool(
                &mut lookup,
                ENV_PRESENCE_UNLOAD_MODEL_WHEN_IDLE,
            )?
            .unwrap_or(false),
            presence_person_confidence_threshold: optional_f32(
                &mut lookup,
                ENV_PRESENCE_PERSON_CONFIDENCE_THRESHOLD,
            )?
            .unwrap_or(DEFAULT_PRESENCE_PERSON_CONFIDENCE_THRESHOLD),
            presence_person_detector_model,
            presence_person_suspect_fps: optional_f32(
                &mut lookup,
                ENV_PRESENCE_PERSON_SUSPECT_FPS,
            )?
            .unwrap_or(DEFAULT_PRESENCE_PERSON_SUSPECT_FPS),
            presence_absent_required_frames: optional_u32(
                &mut lookup,
                ENV_PRESENCE_ABSENT_REQUIRED_FRAMES,
            )?
            .unwrap_or(DEFAULT_PRESENCE_ABSENT_REQUIRED_FRAMES),
            presence_boundary_margin_ratio: optional_f32(
                &mut lookup,
                ENV_PRESENCE_BOUNDARY_MARGIN_RATIO,
            )?
            .unwrap_or(DEFAULT_PRESENCE_BOUNDARY_MARGIN_RATIO),
            presence_movement_delta_ratio: optional_f32(
                &mut lookup,
                ENV_PRESENCE_MOVEMENT_DELTA_RATIO,
            )?
            .unwrap_or(DEFAULT_PRESENCE_MOVEMENT_DELTA_RATIO),
            presence_person_model_path: optional_path_or_default(
                &mut lookup,
                ENV_PRESENCE_PERSON_MODEL_PATH,
                default_person_model_path,
            ),
            presence_person_model_config_path: optional_path(
                &mut lookup,
                ENV_PRESENCE_PERSON_MODEL_CONFIG_PATH,
            )
            .or(default_person_model_config_path),
            presence_person_debug_output_dir: optional_path(
                &mut lookup,
                ENV_PRESENCE_PERSON_DEBUG_OUTPUT_DIR,
            ),
            presence_pose_bridge_dll_path: optional_path_or_default(
                &mut lookup,
                ENV_PRESENCE_POSE_BRIDGE_DLL_PATH,
                DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH,
            ),
            presence_pose_model_path: optional_path_or_default(
                &mut lookup,
                ENV_PRESENCE_POSE_MODEL_PATH,
                DEFAULT_PRESENCE_POSE_MODEL_PATH,
            ),
            presence_pose_min_landmark_visibility: optional_f32(
                &mut lookup,
                ENV_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY,
            )?
            .unwrap_or(DEFAULT_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY),
            presence_pose_min_landmark_presence: optional_f32(
                &mut lookup,
                ENV_PRESENCE_POSE_MIN_LANDMARK_PRESENCE,
            )?
            .unwrap_or(DEFAULT_PRESENCE_POSE_MIN_LANDMARK_PRESENCE),
        })
    }

    fn resolve_relative_paths_against_install_dir(&mut self, install_dir: &Path) {
        resolve_relative_path_against_install_dir(
            &mut self.presence_person_model_path,
            install_dir,
        );
        if let Some(config_path) = &mut self.presence_person_model_config_path {
            resolve_relative_path_against_install_dir(config_path, install_dir);
        }
        if let Some(debug_output_dir) = &mut self.presence_person_debug_output_dir {
            resolve_relative_path_against_install_dir(debug_output_dir, install_dir);
        }
        resolve_relative_path_against_install_dir(
            &mut self.presence_pose_bridge_dll_path,
            install_dir,
        );
        resolve_relative_path_against_install_dir(&mut self.presence_pose_model_path, install_dir);
    }
}

impl ServiceAuthConfig {
    pub fn from_environment() -> Result<Self, ProtocolError> {
        let mut config = Self::from_lookup(|env_name| {
            std::env::var(env_name)
                .ok()
                .or_else(|| registry_value_for_env_name(env_name))
        })?;
        if let Some(install_dir) = current_exe_dir() {
            config.resolve_relative_paths_against_install_dir(&install_dir);
        }
        Ok(config)
    }

    fn from_lookup(
        mut lookup: impl FnMut(&'static str) -> Option<String>,
    ) -> Result<Self, ProtocolError> {
        let auth_mode = lookup(ENV_AUTH_MODE).unwrap_or_else(|| AUTH_MODE_MANUAL_TEST.to_owned());
        match auth_mode.as_str() {
            "" | AUTH_MODE_MANUAL_TEST => Ok(Self {
                auth_mode: ServiceAuthMode::ManualTestOnly,
            }),
            AUTH_MODE_LOCAL_CAMERA => Ok(Self {
                auth_mode: ServiceAuthMode::LocalCamera(Box::new(LocalCameraAuthConfig {
                    face_template_path: required_path(&mut lookup, ENV_FACE_TEMPLATE_PATH)?,
                    camera_id: CameraId(
                        lookup(ENV_CAMERA_ID).unwrap_or_else(|| CameraId::from_index(0).0),
                    ),
                    camera_config: OpenCvCameraProviderConfig {
                        max_camera_index: DEFAULT_MAX_CAMERA_INDEX,
                        requested_frame_width: optional_u32(&mut lookup, ENV_FRAME_WIDTH)?,
                        requested_frame_height: optional_u32(&mut lookup, ENV_FRAME_HEIGHT)?,
                        preferred_backend: None,
                    },
                    yunet_model_path: optional_path_or_default(
                        &mut lookup,
                        ENV_YUNET_MODEL_PATH,
                        DEFAULT_YUNET_MODEL_PATH,
                    ),
                    sface_model_path: optional_path_or_default(
                        &mut lookup,
                        ENV_SFACE_MODEL_PATH,
                        DEFAULT_SFACE_MODEL_PATH,
                    ),
                    minifasnet_config: MiniFasNetLivenessProviderConfig {
                        model_path: optional_path_or_default(
                            &mut lookup,
                            ENV_MINIFASNET_MODEL_PATH,
                            DEFAULT_MINIFASNET_MODEL_PATH,
                        ),
                        crop_scale: optional_f32(&mut lookup, ENV_MINIFASNET_CROP_SCALE)?
                            .unwrap_or(DEFAULT_MINIFASNET_CROP_SCALE),
                        input_width: 80,
                        input_height: 80,
                        min_live_score: optional_f32(&mut lookup, ENV_MINIFASNET_MIN_LIVE_SCORE)?
                            .unwrap_or(DEFAULT_MINIFASNET_MIN_LIVE_SCORE),
                        min_spoof_score: optional_f32(&mut lookup, ENV_MINIFASNET_MIN_SPOOF_SCORE)?
                            .unwrap_or(DEFAULT_MINIFASNET_MIN_SPOOF_SCORE),
                        reject_on_model_spoof: true,
                    },
                    minifasnet_max_spoof_frame_ratio: optional_f32(
                        &mut lookup,
                        ENV_MINIFASNET_MAX_SPOOF_FRAME_RATIO,
                    )?
                    .unwrap_or(DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO),
                    max_auth_frames: optional_u32(&mut lookup, ENV_MAX_AUTH_FRAMES)?
                        .unwrap_or(DEFAULT_MAX_AUTH_FRAMES),
                    required_consecutive_match_count: optional_u32(
                        &mut lookup,
                        ENV_REQUIRED_CONSECUTIVE,
                    )?
                    .unwrap_or(DEFAULT_REQUIRED_CONSECUTIVE_MATCH_COUNT),
                    match_threshold: optional_f32(&mut lookup, ENV_MATCH_THRESHOLD)?
                        .unwrap_or(DEFAULT_SERVICE_FACE_MATCH_THRESHOLD),
                })),
            }),
            _ => Err(ProtocolError::InvalidMessage),
        }
    }

    fn resolve_relative_paths_against_install_dir(&mut self, install_dir: &Path) {
        let ServiceAuthMode::LocalCamera(local_camera_config) = &mut self.auth_mode else {
            return;
        };
        resolve_relative_path_against_install_dir(
            &mut local_camera_config.face_template_path,
            install_dir,
        );
        resolve_relative_path_against_install_dir(
            &mut local_camera_config.yunet_model_path,
            install_dir,
        );
        resolve_relative_path_against_install_dir(
            &mut local_camera_config.sface_model_path,
            install_dir,
        );
        resolve_relative_path_against_install_dir(
            &mut local_camera_config.minifasnet_config.model_path,
            install_dir,
        );
    }
}

fn registry_value_for_env_name(env_name: &'static str) -> Option<String> {
    service_registry::read_string_value(
        SERVICE_CONFIG_REGISTRY_PATH,
        registry_value_name(env_name)?,
    )
}

fn registry_value_name(env_name: &'static str) -> Option<&'static str> {
    match env_name {
        ENV_AUTH_MODE => Some(REG_AUTH_MODE),
        ENV_FACE_TEMPLATE_PATH => Some(REG_FACE_TEMPLATE_PATH),
        ENV_CAMERA_ID => Some(REG_CAMERA_ID),
        ENV_YUNET_MODEL_PATH => Some(REG_YUNET_MODEL_PATH),
        ENV_SFACE_MODEL_PATH => Some(REG_SFACE_MODEL_PATH),
        ENV_MINIFASNET_MODEL_PATH => Some(REG_MINIFASNET_MODEL_PATH),
        ENV_MINIFASNET_CROP_SCALE => Some(REG_MINIFASNET_CROP_SCALE),
        ENV_MINIFASNET_MIN_LIVE_SCORE => Some(REG_MINIFASNET_MIN_LIVE_SCORE),
        ENV_MINIFASNET_MIN_SPOOF_SCORE => Some(REG_MINIFASNET_MIN_SPOOF_SCORE),
        ENV_MINIFASNET_MAX_SPOOF_FRAME_RATIO => Some(REG_MINIFASNET_MAX_SPOOF_FRAME_RATIO),
        ENV_FRAME_WIDTH => Some(REG_FRAME_WIDTH),
        ENV_FRAME_HEIGHT => Some(REG_FRAME_HEIGHT),
        ENV_MAX_AUTH_FRAMES => Some(REG_MAX_AUTH_FRAMES),
        ENV_REQUIRED_CONSECUTIVE => Some(REG_REQUIRED_CONSECUTIVE),
        ENV_MATCH_THRESHOLD => Some(REG_MATCH_THRESHOLD),
        ENV_PRESENCE_LOCK_ENABLED => Some(REG_PRESENCE_LOCK_ENABLED),
        ENV_PRESENCE_OWNER_MATCH_THRESHOLD => Some(REG_PRESENCE_OWNER_MATCH_THRESHOLD),
        ENV_PRESENCE_DETECTOR_KIND => Some(REG_PRESENCE_DETECTOR_KIND),
        ENV_PRESENCE_TRACKING_MODE => Some(REG_PRESENCE_TRACKING_MODE),
        ENV_PRESENCE_DETECTOR_FPS => Some(REG_PRESENCE_DETECTOR_FPS),
        ENV_PRESENCE_UNLOAD_MODEL_WHEN_IDLE => Some(REG_PRESENCE_UNLOAD_MODEL_WHEN_IDLE),
        ENV_PRESENCE_PERSON_CONFIDENCE_THRESHOLD => Some(REG_PRESENCE_PERSON_CONFIDENCE_THRESHOLD),
        ENV_PRESENCE_PERSON_DETECTOR_MODEL => Some(REG_PRESENCE_PERSON_DETECTOR_MODEL),
        ENV_PRESENCE_PERSON_SUSPECT_FPS => Some(REG_PRESENCE_PERSON_SUSPECT_FPS),
        ENV_PRESENCE_ABSENT_REQUIRED_FRAMES => Some(REG_PRESENCE_ABSENT_REQUIRED_FRAMES),
        ENV_PRESENCE_BOUNDARY_MARGIN_RATIO => Some(REG_PRESENCE_BOUNDARY_MARGIN_RATIO),
        ENV_PRESENCE_MOVEMENT_DELTA_RATIO => Some(REG_PRESENCE_MOVEMENT_DELTA_RATIO),
        ENV_PRESENCE_PERSON_MODEL_PATH => Some(REG_PRESENCE_PERSON_MODEL_PATH),
        ENV_PRESENCE_PERSON_MODEL_CONFIG_PATH => Some(REG_PRESENCE_PERSON_MODEL_CONFIG_PATH),
        ENV_PRESENCE_PERSON_DEBUG_OUTPUT_DIR => Some(REG_PRESENCE_PERSON_DEBUG_OUTPUT_DIR),
        ENV_PRESENCE_POSE_BRIDGE_DLL_PATH => Some(REG_PRESENCE_POSE_BRIDGE_DLL_PATH),
        ENV_PRESENCE_POSE_MODEL_PATH => Some(REG_PRESENCE_POSE_MODEL_PATH),
        ENV_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY => {
            Some(REG_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY)
        }
        ENV_PRESENCE_POSE_MIN_LANDMARK_PRESENCE => Some(REG_PRESENCE_POSE_MIN_LANDMARK_PRESENCE),
        _ => None,
    }
}

fn optional_presence_person_detector_model(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<PresencePersonDetectorModel>, ProtocolError> {
    lookup(env_name)
        .map(|value| match value.trim() {
            "" | PRESENCE_PERSON_DETECTOR_MODEL_MOBILENET_SSD => {
                Ok(PresencePersonDetectorModel::MobileNetSsd)
            }
            PRESENCE_PERSON_DETECTOR_MODEL_YOLOV8_ONNX => {
                Ok(PresencePersonDetectorModel::YoloV8Onnx)
            }
            PRESENCE_PERSON_DETECTOR_MODEL_ORT_YOLOV8_ONNX => {
                Ok(PresencePersonDetectorModel::OrtYoloV8Onnx)
            }
            _ => Err(ProtocolError::InvalidMessage),
        })
        .transpose()
}

fn optional_presence_detector_kind(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<PresenceDetectorKind>, ProtocolError> {
    lookup(env_name)
        .map(|value| match value.trim() {
            "" | PRESENCE_DETECTOR_KIND_FACE_OWNER_MATCH => {
                Ok(PresenceDetectorKind::FaceOwnerMatch)
            }
            PRESENCE_DETECTOR_KIND_OPENCV_DNN_PERSON => Ok(PresenceDetectorKind::OpenCvDnnPerson),
            PRESENCE_DETECTOR_KIND_MEDIAPIPE_POSE_LITE => {
                Ok(PresenceDetectorKind::MediaPipePoseLite)
            }
            _ => Err(ProtocolError::InvalidMessage),
        })
        .transpose()
}

fn optional_presence_tracking_mode(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<PresenceTrackingMode>, ProtocolError> {
    lookup(env_name)
        .map(|value| match value.trim() {
            "" | PRESENCE_TRACKING_MODE_FACE_POLICY => Ok(PresenceTrackingMode::FacePolicy),
            PRESENCE_TRACKING_MODE_CONTINUOUS_LOW_FPS => Ok(PresenceTrackingMode::ContinuousLowFps),
            _ => Err(ProtocolError::InvalidMessage),
        })
        .transpose()
}

fn optional_bool(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<bool>, ProtocolError> {
    lookup(env_name)
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "enabled" | "on" => Ok(true),
            "0" | "false" | "no" | "disabled" | "off" => Ok(false),
            _ => Err(ProtocolError::InvalidMessage),
        })
        .transpose()
}

fn required_path(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<PathBuf, ProtocolError> {
    lookup(env_name)
        .map(PathBuf::from)
        .ok_or(ProtocolError::InvalidMessage)
}

fn optional_path_or_default(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
    default_path: &str,
) -> PathBuf {
    lookup(env_name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn optional_path(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Option<PathBuf> {
    lookup(env_name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn resolve_relative_path_against_install_dir(path: &mut PathBuf, install_dir: &Path) {
    if path.is_relative() {
        *path = install_dir.join(&path);
    }
}

fn optional_u32(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<u32>, ProtocolError> {
    lookup(env_name)
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| ProtocolError::InvalidMessage)
}

fn optional_f32(
    lookup: &mut impl FnMut(&'static str) -> Option<String>,
    env_name: &'static str,
) -> Result<Option<f32>, ProtocolError> {
    lookup(env_name)
        .map(|value| value.parse::<f32>())
        .transpose()
        .map_err(|_| ProtocolError::InvalidMessage)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn defaults_to_manual_test_mode_when_auth_mode_is_missing() -> Result<(), ProtocolError> {
        let config = ServiceAuthConfig::from_lookup(|_| None)?;

        assert_eq!(config.auth_mode, ServiceAuthMode::ManualTestOnly);
        Ok(())
    }

    #[test]
    fn env_name_maps_to_explicit_registry_value_name() {
        assert_eq!(
            registry_value_name(ENV_REQUIRED_CONSECUTIVE),
            Some(REG_REQUIRED_CONSECUTIVE)
        );
        assert_eq!(
            registry_value_name(ENV_MATCH_THRESHOLD),
            Some(REG_MATCH_THRESHOLD)
        );
    }

    #[test]
    fn parses_local_camera_config_with_explicit_names() -> Result<(), ProtocolError> {
        let values = HashMap::from([
            (ENV_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA),
            (ENV_FACE_TEMPLATE_PATH, r"D:\templates\face.json"),
            (ENV_CAMERA_ID, "opencv-index:2"),
            (ENV_YUNET_MODEL_PATH, r"D:\models\yunet.onnx"),
            (ENV_SFACE_MODEL_PATH, r"D:\models\sface.onnx"),
            (ENV_MINIFASNET_MODEL_PATH, r"D:\models\minifasnet.onnx"),
            (ENV_MINIFASNET_CROP_SCALE, "1.3"),
            (ENV_MINIFASNET_MIN_LIVE_SCORE, "0.81"),
            (ENV_MINIFASNET_MIN_SPOOF_SCORE, "0.71"),
            (ENV_MINIFASNET_MAX_SPOOF_FRAME_RATIO, "0.45"),
            (ENV_FRAME_WIDTH, "640"),
            (ENV_FRAME_HEIGHT, "480"),
            (ENV_MAX_AUTH_FRAMES, "12"),
            (ENV_REQUIRED_CONSECUTIVE, "3"),
            (ENV_MATCH_THRESHOLD, "0.42"),
        ]);

        let config = ServiceAuthConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        let ServiceAuthMode::LocalCamera(local_camera) = config.auth_mode else {
            return Err(ProtocolError::InvalidMessage);
        };
        assert_eq!(
            local_camera.face_template_path,
            PathBuf::from(r"D:\templates\face.json")
        );
        assert_eq!(
            local_camera.camera_id,
            CameraId("opencv-index:2".to_owned())
        );
        assert_eq!(local_camera.camera_config.requested_frame_width, Some(640));
        assert_eq!(local_camera.camera_config.requested_frame_height, Some(480));
        assert_eq!(local_camera.max_auth_frames, 12);
        assert_eq!(local_camera.required_consecutive_match_count, 3);
        assert_eq!(local_camera.match_threshold, 0.42);
        assert_eq!(
            local_camera.minifasnet_config.model_path,
            PathBuf::from(r"D:\models\minifasnet.onnx")
        );
        assert_eq!(local_camera.minifasnet_config.crop_scale, 1.3);
        assert_eq!(local_camera.minifasnet_config.min_live_score, 0.81);
        assert_eq!(local_camera.minifasnet_config.min_spoof_score, 0.71);
        assert_eq!(local_camera.minifasnet_max_spoof_frame_ratio, 0.45);
        assert!(local_camera.minifasnet_config.reject_on_model_spoof);
        Ok(())
    }

    #[test]
    fn local_camera_mode_requires_template_path() {
        let values = HashMap::from([(ENV_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA)]);

        let result = ServiceAuthConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        });

        assert_eq!(result, Err(ProtocolError::InvalidMessage));
    }

    #[test]
    fn local_camera_mode_defaults_to_project_initial_threshold() -> Result<(), ProtocolError> {
        let values = HashMap::from([
            (ENV_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA),
            (ENV_FACE_TEMPLATE_PATH, r"D:\templates\face.json"),
        ]);

        let config = ServiceAuthConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        let ServiceAuthMode::LocalCamera(local_camera) = config.auth_mode else {
            return Err(ProtocolError::InvalidMessage);
        };
        assert_eq!(
            local_camera.match_threshold,
            DEFAULT_SERVICE_FACE_MATCH_THRESHOLD
        );
        assert_eq!(
            local_camera.minifasnet_config.model_path,
            PathBuf::from(DEFAULT_MINIFASNET_MODEL_PATH)
        );
        assert_eq!(
            local_camera.minifasnet_config.crop_scale,
            DEFAULT_MINIFASNET_CROP_SCALE
        );
        assert_eq!(
            local_camera.minifasnet_max_spoof_frame_ratio,
            DEFAULT_MINIFASNET_MAX_SPOOF_FRAME_RATIO
        );
        Ok(())
    }

    #[test]
    fn invalid_numeric_config_is_rejected() {
        let values = HashMap::from([
            (ENV_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA),
            (ENV_FACE_TEMPLATE_PATH, r"D:\templates\face.json"),
            (ENV_MAX_AUTH_FRAMES, "many"),
        ]);

        let result = ServiceAuthConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        });

        assert_eq!(result, Err(ProtocolError::InvalidMessage));
    }

    #[test]
    fn presence_config_defaults_to_disabled_when_missing() -> Result<(), ProtocolError> {
        let config = ServicePresenceConfig::from_lookup(|_| None)?;

        assert!(!config.presence_lock_enabled);
        assert_eq!(
            config.presence_owner_match_threshold,
            DEFAULT_PRESENCE_OWNER_MATCH_THRESHOLD
        );
        assert_eq!(
            config.presence_detector_kind,
            PresenceDetectorKind::OpenCvDnnPerson
        );
        assert_eq!(
            config.presence_tracking_mode,
            PresenceTrackingMode::ContinuousLowFps
        );
        assert_eq!(config.presence_detector_fps, DEFAULT_PRESENCE_DETECTOR_FPS);
        assert!(!config.presence_unload_model_when_idle);
        assert_eq!(
            config.presence_person_detector_model,
            PresencePersonDetectorModel::OrtYoloV8Onnx
        );
        assert_eq!(
            config.presence_person_model_path,
            PathBuf::from(DEFAULT_PRESENCE_YOLOV8_PERSON_MODEL_PATH)
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
        Ok(())
    }

    #[test]
    fn presence_config_parses_enabled_and_threshold() -> Result<(), ProtocolError> {
        let values = HashMap::from([
            (ENV_PRESENCE_LOCK_ENABLED, "true"),
            (ENV_PRESENCE_OWNER_MATCH_THRESHOLD, "0.47"),
            (ENV_PRESENCE_DETECTOR_KIND, "opencv-dnn-person"),
            (ENV_PRESENCE_TRACKING_MODE, "continuous-low-fps"),
            (ENV_PRESENCE_DETECTOR_FPS, "3.0"),
            (ENV_PRESENCE_UNLOAD_MODEL_WHEN_IDLE, "false"),
            (ENV_PRESENCE_PERSON_CONFIDENCE_THRESHOLD, "0.62"),
            (ENV_PRESENCE_PERSON_DETECTOR_MODEL, "yolov8-onnx"),
            (ENV_PRESENCE_PERSON_SUSPECT_FPS, "5.0"),
            (ENV_PRESENCE_ABSENT_REQUIRED_FRAMES, "9"),
            (ENV_PRESENCE_BOUNDARY_MARGIN_RATIO, "0.18"),
            (ENV_PRESENCE_MOVEMENT_DELTA_RATIO, "0.07"),
            (ENV_PRESENCE_PERSON_MODEL_PATH, r"D:\models\person.onnx"),
            (ENV_PRESENCE_POSE_BRIDGE_DLL_PATH, r"D:\native\bridge.dll"),
            (ENV_PRESENCE_POSE_MODEL_PATH, r"D:\models\pose.task"),
            (ENV_PRESENCE_POSE_MIN_LANDMARK_VISIBILITY, "0.41"),
            (ENV_PRESENCE_POSE_MIN_LANDMARK_PRESENCE, "0.42"),
            (
                ENV_PRESENCE_PERSON_DEBUG_OUTPUT_DIR,
                r"D:\debug\presence-person",
            ),
        ]);

        let config = ServicePresenceConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        assert!(config.presence_lock_enabled);
        assert_eq!(config.presence_owner_match_threshold, 0.47);
        assert_eq!(
            config.presence_detector_kind,
            PresenceDetectorKind::OpenCvDnnPerson
        );
        assert_eq!(
            config.presence_tracking_mode,
            PresenceTrackingMode::ContinuousLowFps
        );
        assert_eq!(config.presence_detector_fps, 3.0);
        assert!(!config.presence_unload_model_when_idle);
        assert_eq!(config.presence_person_confidence_threshold, 0.62);
        assert_eq!(
            config.presence_person_detector_model,
            PresencePersonDetectorModel::YoloV8Onnx
        );
        assert_eq!(config.presence_person_suspect_fps, 5.0);
        assert_eq!(config.presence_absent_required_frames, 9);
        assert_eq!(config.presence_boundary_margin_ratio, 0.18);
        assert_eq!(config.presence_movement_delta_ratio, 0.07);
        assert_eq!(
            config.presence_person_model_path,
            PathBuf::from(r"D:\models\person.onnx")
        );
        assert_eq!(config.presence_person_model_config_path, None);
        assert_eq!(
            config.presence_person_debug_output_dir,
            Some(PathBuf::from(r"D:\debug\presence-person"))
        );
        assert_eq!(
            config.presence_pose_bridge_dll_path,
            PathBuf::from(r"D:\native\bridge.dll")
        );
        assert_eq!(
            config.presence_pose_model_path,
            PathBuf::from(r"D:\models\pose.task")
        );
        assert_eq!(config.presence_pose_min_landmark_visibility, 0.41);
        assert_eq!(config.presence_pose_min_landmark_presence, 0.42);
        Ok(())
    }

    #[test]
    fn presence_config_parses_mediapipe_pose_lite_detector_kind() -> Result<(), ProtocolError> {
        let values = HashMap::from([(
            ENV_PRESENCE_DETECTOR_KIND,
            PRESENCE_DETECTOR_KIND_MEDIAPIPE_POSE_LITE,
        )]);

        let config = ServicePresenceConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        assert_eq!(
            config.presence_detector_kind,
            PresenceDetectorKind::MediaPipePoseLite
        );
        Ok(())
    }

    #[test]
    fn yolo_person_detector_model_defaults_to_yolo_model_path() -> Result<(), ProtocolError> {
        let values = HashMap::from([(ENV_PRESENCE_PERSON_DETECTOR_MODEL, "yolov8-onnx")]);

        let config = ServicePresenceConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        assert_eq!(
            config.presence_person_detector_model,
            PresencePersonDetectorModel::YoloV8Onnx
        );
        assert_eq!(
            config.presence_person_model_path,
            PathBuf::from(DEFAULT_PRESENCE_YOLOV8_PERSON_MODEL_PATH)
        );
        assert_eq!(config.presence_person_model_config_path, None);
        Ok(())
    }

    #[test]
    fn presence_runtime_paths_resolve_relative_defaults_under_install_dir()
    -> Result<(), ProtocolError> {
        let install_dir = PathBuf::from(r"D:\tools\WinFaceUnlock");
        let mut config = ServicePresenceConfig::from_lookup(|_| None)?;

        config.resolve_relative_paths_against_install_dir(&install_dir);

        assert_eq!(
            config.presence_person_model_path,
            install_dir.join(DEFAULT_PRESENCE_YOLOV8_PERSON_MODEL_PATH)
        );
        assert_eq!(
            config.presence_pose_bridge_dll_path,
            install_dir.join(DEFAULT_PRESENCE_POSE_BRIDGE_DLL_PATH)
        );
        assert_eq!(
            config.presence_pose_model_path,
            install_dir.join(DEFAULT_PRESENCE_POSE_MODEL_PATH)
        );
        Ok(())
    }

    #[test]
    fn auth_runtime_paths_resolve_relative_defaults_under_install_dir() -> Result<(), ProtocolError>
    {
        let install_dir = PathBuf::from(r"D:\tools\WinFaceUnlock");
        let values = HashMap::from([
            (ENV_AUTH_MODE, AUTH_MODE_LOCAL_CAMERA),
            (
                ENV_FACE_TEMPLATE_PATH,
                r"face-enrollment\selected_templates.json",
            ),
        ]);
        let mut config = ServiceAuthConfig::from_lookup(|env_name| {
            values.get(env_name).map(|value| value.to_string())
        })?;

        config.resolve_relative_paths_against_install_dir(&install_dir);

        let ServiceAuthMode::LocalCamera(local_camera_config) = config.auth_mode else {
            return Err(ProtocolError::InvalidMessage);
        };
        assert_eq!(
            local_camera_config.face_template_path,
            install_dir.join(r"face-enrollment\selected_templates.json")
        );
        assert_eq!(
            local_camera_config.yunet_model_path,
            install_dir.join(DEFAULT_YUNET_MODEL_PATH)
        );
        assert_eq!(
            local_camera_config.sface_model_path,
            install_dir.join(DEFAULT_SFACE_MODEL_PATH)
        );
        assert_eq!(
            local_camera_config.minifasnet_config.model_path,
            install_dir.join(DEFAULT_MINIFASNET_MODEL_PATH)
        );
        Ok(())
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod service_registry {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        },
    };

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

    fn to_wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod service_registry {
    pub fn read_string_value(_path: &str, _value_name: &str) -> Option<String> {
        None
    }
}
