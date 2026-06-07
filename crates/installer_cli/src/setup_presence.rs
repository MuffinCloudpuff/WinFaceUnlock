use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

use setup_api::ConfigurePresenceLockPayload;

use crate::service_registry::ServicePresenceRegistryPatch;

const DETECTOR_KIND_FACE_OWNER_MATCH: &str = "face-owner-match";
const DETECTOR_KIND_OPENCV_DNN_PERSON: &str = "opencv-dnn-person";
const TRACKING_MODE_FACE_POLICY: &str = "face-policy";
const TRACKING_MODE_CONTINUOUS_LOW_FPS: &str = "continuous-low-fps";
const PERSON_DETECTOR_MODEL_MOBILENET_SSD: &str = "mobilenet-ssd";
const PERSON_DETECTOR_MODEL_YOLOV8_ONNX: &str = "yolov8-onnx";

pub fn build_presence_patch(
    payload: &ConfigurePresenceLockPayload,
) -> Result<ServicePresenceRegistryPatch, SetupPresenceError> {
    let mut patch = ServicePresenceRegistryPatch {
        presence_lock_enabled: payload.presence_lock_enabled,
        presence_owner_match_threshold: payload.presence_owner_match_threshold,
        presence_detector_fps: payload.detector_fps,
        presence_unload_model_when_idle: payload.unload_model_when_idle,
        presence_person_confidence_threshold: payload.person_confidence_threshold,
        presence_person_suspect_fps: payload.person_suspect_fps,
        presence_absent_required_frames: payload.absent_required_frames,
        presence_boundary_margin_ratio: payload.boundary_margin_ratio,
        presence_movement_delta_ratio: payload.movement_delta_ratio,
        ..ServicePresenceRegistryPatch::default()
    };

    if let Some(detector_kind) = &payload.detector_kind {
        validate_detector_kind(detector_kind)?;
        patch.presence_detector_kind = Some(detector_kind.clone());
    }
    if let Some(tracking_mode) = &payload.tracking_mode {
        validate_tracking_mode(tracking_mode)?;
        patch.presence_tracking_mode = Some(tracking_mode.clone());
    }
    if let Some(person_detector_model) = &payload.person_detector_model {
        apply_person_detector_model_defaults(&mut patch, payload, person_detector_model)?;
    }
    if let Some(person_model_relative_path) = &payload.person_model_relative_path {
        patch.presence_person_model_path = Some(resolve_install_relative_path(
            required_install_dir(payload)?,
            person_model_relative_path,
        )?);
    }
    if let Some(person_model_config_relative_path) = &payload.person_model_config_relative_path {
        if payload.clear_person_model_config {
            return Err(SetupPresenceError::ConflictingModelConfigOptions);
        }
        patch.presence_person_model_config_path = Some(Some(resolve_install_relative_path(
            required_install_dir(payload)?,
            person_model_config_relative_path,
        )?));
    } else if payload.clear_person_model_config {
        patch.presence_person_model_config_path = Some(None);
    }
    if let Some(person_debug_output_dir) = &payload.person_debug_output_dir {
        if payload.clear_person_debug_output_dir {
            return Err(SetupPresenceError::ConflictingDebugOutputOptions);
        }
        patch.presence_person_debug_output_dir = Some(Some(person_debug_output_dir.clone()));
    } else if payload.clear_person_debug_output_dir {
        patch.presence_person_debug_output_dir = Some(None);
    }

    if !patch_has_updates(&patch) {
        return Err(SetupPresenceError::NoPresenceUpdates);
    }

    Ok(patch)
}

fn apply_person_detector_model_defaults(
    patch: &mut ServicePresenceRegistryPatch,
    payload: &ConfigurePresenceLockPayload,
    person_detector_model: &str,
) -> Result<(), SetupPresenceError> {
    validate_person_detector_model(person_detector_model)?;
    patch.presence_person_detector_model = Some(person_detector_model.to_owned());

    match person_detector_model {
        PERSON_DETECTOR_MODEL_MOBILENET_SSD => {
            let install_dir = required_install_dir(payload)?;
            patch.presence_person_model_path = Some(resolve_install_relative_path(
                install_dir,
                Path::new(r"models\MobileNetSSD_deploy.caffemodel"),
            )?);
            patch.presence_person_model_config_path = Some(Some(resolve_install_relative_path(
                install_dir,
                Path::new(r"models\MobileNetSSD_deploy.prototxt"),
            )?));
        }
        PERSON_DETECTOR_MODEL_YOLOV8_ONNX => {
            patch.presence_person_model_path = Some(resolve_install_relative_path(
                required_install_dir(payload)?,
                Path::new(r"models\yolov8n.onnx"),
            )?);
            patch.presence_person_model_config_path = Some(None);
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn required_install_dir(
    payload: &ConfigurePresenceLockPayload,
) -> Result<&Path, SetupPresenceError> {
    let install_dir = payload
        .install_dir
        .as_deref()
        .ok_or(SetupPresenceError::InstallDirRequired)?;
    validate_install_dir(install_dir)?;
    Ok(install_dir)
}

fn validate_install_dir(install_dir: &Path) -> Result<(), SetupPresenceError> {
    if install_dir.as_os_str().is_empty() || !install_dir.is_absolute() {
        return Err(SetupPresenceError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    if install_dir.parent().is_none() || install_dir.file_name().is_none() {
        return Err(SetupPresenceError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    Ok(())
}

fn resolve_install_relative_path(
    install_dir: &Path,
    relative_path: &Path,
) -> Result<PathBuf, SetupPresenceError> {
    validate_relative_path(relative_path)?;
    Ok(install_dir.join(relative_path))
}

fn validate_relative_path(relative_path: &Path) -> Result<(), SetupPresenceError> {
    if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
        return Err(SetupPresenceError::InvalidRelativePath(
            relative_path.to_path_buf(),
        ));
    }
    for component in relative_path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(SetupPresenceError::InvalidRelativePath(
                    relative_path.to_path_buf(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_detector_kind(value: &str) -> Result<(), SetupPresenceError> {
    match value {
        DETECTOR_KIND_FACE_OWNER_MATCH | DETECTOR_KIND_OPENCV_DNN_PERSON => Ok(()),
        other => Err(SetupPresenceError::InvalidPresenceValue {
            field_name: "detector_kind",
            value: other.to_owned(),
        }),
    }
}

fn validate_tracking_mode(value: &str) -> Result<(), SetupPresenceError> {
    match value {
        TRACKING_MODE_FACE_POLICY | TRACKING_MODE_CONTINUOUS_LOW_FPS => Ok(()),
        other => Err(SetupPresenceError::InvalidPresenceValue {
            field_name: "tracking_mode",
            value: other.to_owned(),
        }),
    }
}

fn validate_person_detector_model(value: &str) -> Result<(), SetupPresenceError> {
    match value {
        PERSON_DETECTOR_MODEL_MOBILENET_SSD | PERSON_DETECTOR_MODEL_YOLOV8_ONNX => Ok(()),
        other => Err(SetupPresenceError::InvalidPresenceValue {
            field_name: "person_detector_model",
            value: other.to_owned(),
        }),
    }
}

fn patch_has_updates(patch: &ServicePresenceRegistryPatch) -> bool {
    patch.presence_lock_enabled.is_some()
        || patch.presence_owner_match_threshold.is_some()
        || patch.presence_detector_kind.is_some()
        || patch.presence_tracking_mode.is_some()
        || patch.presence_detector_fps.is_some()
        || patch.presence_unload_model_when_idle.is_some()
        || patch.presence_person_confidence_threshold.is_some()
        || patch.presence_person_detector_model.is_some()
        || patch.presence_person_suspect_fps.is_some()
        || patch.presence_absent_required_frames.is_some()
        || patch.presence_boundary_margin_ratio.is_some()
        || patch.presence_movement_delta_ratio.is_some()
        || patch.presence_person_model_path.is_some()
        || patch.presence_person_model_config_path.is_some()
        || patch.presence_person_debug_output_dir.is_some()
}

#[derive(Debug)]
pub enum SetupPresenceError {
    NoPresenceUpdates,
    InstallDirRequired,
    InvalidInstallDir(PathBuf),
    InvalidRelativePath(PathBuf),
    ConflictingModelConfigOptions,
    ConflictingDebugOutputOptions,
    InvalidPresenceValue {
        field_name: &'static str,
        value: String,
    },
}

impl SetupPresenceError {
    pub fn is_invalid_install_dir(&self) -> bool {
        matches!(self, Self::InvalidInstallDir(_) | Self::InstallDirRequired)
    }
}

impl fmt::Display for SetupPresenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPresenceUpdates => write!(formatter, "no presence settings were provided"),
            Self::InstallDirRequired => write!(
                formatter,
                "install_dir is required when configuring presence model paths"
            ),
            Self::InvalidInstallDir(path) => {
                write!(formatter, "invalid install directory: {}", path.display())
            }
            Self::InvalidRelativePath(path) => {
                write!(
                    formatter,
                    "invalid presence relative path: {}",
                    path.display()
                )
            }
            Self::ConflictingModelConfigOptions => write!(
                formatter,
                "person_model_config_relative_path and clear_person_model_config conflict"
            ),
            Self::ConflictingDebugOutputOptions => write!(
                formatter,
                "person_debug_output_dir and clear_person_debug_output_dir conflict"
            ),
            Self::InvalidPresenceValue { field_name, value } => {
                write!(formatter, "invalid presence {field_name}: {value}")
            }
        }
    }
}

impl std::error::Error for SetupPresenceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_patch_updates_only_presence_lock_without_install_dir()
    -> Result<(), SetupPresenceError> {
        let patch = build_presence_patch(&ConfigurePresenceLockPayload {
            install_dir: None,
            presence_lock_enabled: Some(true),
            presence_owner_match_threshold: None,
            detector_kind: Some(DETECTOR_KIND_FACE_OWNER_MATCH.to_owned()),
            tracking_mode: None,
            detector_fps: None,
            unload_model_when_idle: None,
            person_confidence_threshold: None,
            person_detector_model: None,
            person_suspect_fps: None,
            absent_required_frames: None,
            boundary_margin_ratio: None,
            movement_delta_ratio: None,
            person_model_relative_path: None,
            person_model_config_relative_path: None,
            clear_person_model_config: false,
            person_debug_output_dir: None,
            clear_person_debug_output_dir: false,
        })?;

        assert_eq!(patch.presence_lock_enabled, Some(true));
        assert_eq!(
            patch.presence_detector_kind.as_deref(),
            Some(DETECTOR_KIND_FACE_OWNER_MATCH)
        );
        assert_eq!(patch.presence_person_model_path, None);
        Ok(())
    }

    #[test]
    fn yolov8_defaults_resolve_under_install_dir() -> Result<(), SetupPresenceError> {
        let install_dir = PathBuf::from(r"D:\Apps\WinFaceUnlock");
        let patch = build_presence_patch(&ConfigurePresenceLockPayload {
            install_dir: Some(install_dir.clone()),
            presence_lock_enabled: None,
            presence_owner_match_threshold: None,
            detector_kind: None,
            tracking_mode: None,
            detector_fps: None,
            unload_model_when_idle: None,
            person_confidence_threshold: None,
            person_detector_model: Some(PERSON_DETECTOR_MODEL_YOLOV8_ONNX.to_owned()),
            person_suspect_fps: None,
            absent_required_frames: None,
            boundary_margin_ratio: None,
            movement_delta_ratio: None,
            person_model_relative_path: None,
            person_model_config_relative_path: None,
            clear_person_model_config: false,
            person_debug_output_dir: None,
            clear_person_debug_output_dir: false,
        })?;

        assert_eq!(
            patch.presence_person_model_path,
            Some(install_dir.join(r"models\yolov8n.onnx"))
        );
        assert_eq!(patch.presence_person_model_config_path, Some(None));
        Ok(())
    }

    #[test]
    fn model_relative_path_requires_install_dir() {
        let result = build_presence_patch(&ConfigurePresenceLockPayload {
            install_dir: None,
            presence_lock_enabled: None,
            presence_owner_match_threshold: None,
            detector_kind: None,
            tracking_mode: None,
            detector_fps: None,
            unload_model_when_idle: None,
            person_confidence_threshold: None,
            person_detector_model: None,
            person_suspect_fps: None,
            absent_required_frames: None,
            boundary_margin_ratio: None,
            movement_delta_ratio: None,
            person_model_relative_path: Some(PathBuf::from(r"models\person.onnx")),
            person_model_config_relative_path: None,
            clear_person_model_config: false,
            person_debug_output_dir: None,
            clear_person_debug_output_dir: false,
        });

        assert!(matches!(
            result,
            Err(SetupPresenceError::InstallDirRequired)
        ));
    }
}
