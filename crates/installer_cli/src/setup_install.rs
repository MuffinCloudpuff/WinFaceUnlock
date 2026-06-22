use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

use setup_api::InstallSystemComponentsPayload;
use windows_provider::{
    TILE_VISIBILITY_HIDDEN_UNTIL_READY, TILE_VISIBILITY_VISIBLE, WAKE_AUTH_SOURCE_LOCAL_CAMERA,
    WAKE_AUTH_SOURCE_MANUAL_TEST,
};

use crate::{
    installation::FullInstallPlan, provider_registry::ProviderInstallPlan,
    resource_directory::ResourceDirectoryPlan, service_manager::ServiceInstallPlan,
    service_registry::ServiceAuthRegistryConfig,
};

pub fn build_install_plan(
    payload: &InstallSystemComponentsPayload,
) -> Result<FullInstallPlan, SetupInstallError> {
    validate_install_dir(&payload.install_dir)?;

    let service_binary_path =
        install_path(&payload.install_dir, &payload.service_binary_relative_path)?;
    let provider_binary_path =
        install_path(&payload.install_dir, &payload.provider_binary_relative_path)?;
    validate_required_file("service_binary", &service_binary_path)?;
    validate_required_file("provider_binary", &provider_binary_path)?;

    let auth_config = if payload.configure_local_camera_auth {
        let face_template_path =
            install_path(&payload.install_dir, &payload.face_template_relative_path)?;
        let yunet_model_path =
            install_path(&payload.install_dir, &payload.yunet_model_relative_path)?;
        let sface_model_path =
            install_path(&payload.install_dir, &payload.sface_model_relative_path)?;
        let minifasnet_model_path = install_path(
            &payload.install_dir,
            &payload.minifasnet_model_relative_path,
        )?;
        let presence_person_model_path = install_path(
            &payload.install_dir,
            &payload.presence_person_model_relative_path,
        )?;
        let presence_person_model_config_path = if payload
            .presence_person_model_config_relative_path
            .as_os_str()
            .is_empty()
        {
            None
        } else {
            Some(install_path(
                &payload.install_dir,
                &payload.presence_person_model_config_relative_path,
            )?)
        };
        let presence_pose_bridge_dll_path = install_path(
            &payload.install_dir,
            &payload.presence_pose_bridge_relative_path,
        )?;
        let presence_pose_model_path = install_path(
            &payload.install_dir,
            &payload.presence_pose_model_relative_path,
        )?;

        validate_required_file("face_template", &face_template_path)?;
        validate_required_file("yunet_model", &yunet_model_path)?;
        validate_required_file("sface_model", &sface_model_path)?;
        validate_required_file("minifasnet_model", &minifasnet_model_path)?;

        let mut auth_config = ServiceAuthRegistryConfig::local_camera(
            face_template_path,
            yunet_model_path,
            sface_model_path,
        );
        auth_config.minifasnet_model_path = minifasnet_model_path;
        auth_config.presence_person_model_path = presence_person_model_path;
        auth_config.presence_person_model_config_path = presence_person_model_config_path;
        auth_config.presence_pose_bridge_dll_path = presence_pose_bridge_dll_path;
        auth_config.presence_pose_model_path = presence_pose_model_path;
        auth_config.camera_id = payload.camera_id.clone();
        if let Some(match_threshold) = payload.match_threshold {
            auth_config.match_threshold = match_threshold;
        }
        if let Some(required_consecutive_match_count) = payload.required_consecutive_match_count {
            auth_config.required_consecutive_match_count = required_consecutive_match_count;
        }
        Some(auth_config)
    } else {
        None
    };

    let provider_plan = ProviderInstallPlan::new(provider_binary_path)
        .with_wake_auth_source(validate_wake_auth_source(
            &payload.provider_mode.wake_auth_source,
        )?)
        .with_tile_visibility(validate_tile_visibility(
            &payload.provider_mode.tile_visibility,
        )?)
        .with_auto_wake_on_advise(payload.provider_mode.auto_wake_on_advise);

    Ok(FullInstallPlan {
        service_plan: ServiceInstallPlan::new(service_binary_path),
        provider_plan,
        resource_plan: ResourceDirectoryPlan::from_root_dir(payload.install_dir.clone()),
        auth_config,
        start_service: payload.start_service,
    })
}

fn validate_install_dir(install_dir: &Path) -> Result<(), SetupInstallError> {
    if install_dir.as_os_str().is_empty() || !install_dir.is_absolute() {
        return Err(SetupInstallError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    if install_dir.parent().is_none() || install_dir.file_name().is_none() {
        return Err(SetupInstallError::InvalidInstallDir(
            install_dir.to_path_buf(),
        ));
    }
    Ok(())
}

fn install_path(install_dir: &Path, relative_path: &Path) -> Result<PathBuf, SetupInstallError> {
    validate_install_relative_path(relative_path)?;
    Ok(install_dir.join(relative_path))
}

fn validate_install_relative_path(relative_path: &Path) -> Result<(), SetupInstallError> {
    if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
        return Err(SetupInstallError::InvalidRelativePath(
            relative_path.to_path_buf(),
        ));
    }

    for component in relative_path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(SetupInstallError::InvalidRelativePath(
                    relative_path.to_path_buf(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_required_file(file_id: &'static str, path: &Path) -> Result<(), SetupInstallError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(SetupInstallError::MissingRequiredFile {
            file_id,
            path: path.to_path_buf(),
        })
    }
}

fn validate_wake_auth_source(value: &str) -> Result<&'static str, SetupInstallError> {
    match value {
        WAKE_AUTH_SOURCE_LOCAL_CAMERA => Ok(WAKE_AUTH_SOURCE_LOCAL_CAMERA),
        WAKE_AUTH_SOURCE_MANUAL_TEST => Ok(WAKE_AUTH_SOURCE_MANUAL_TEST),
        other => Err(SetupInstallError::InvalidProviderMode {
            field_name: "wake_auth_source",
            value: other.to_owned(),
        }),
    }
}

fn validate_tile_visibility(value: &str) -> Result<&'static str, SetupInstallError> {
    match value {
        TILE_VISIBILITY_HIDDEN_UNTIL_READY => Ok(TILE_VISIBILITY_HIDDEN_UNTIL_READY),
        TILE_VISIBILITY_VISIBLE => Ok(TILE_VISIBILITY_VISIBLE),
        other => Err(SetupInstallError::InvalidProviderMode {
            field_name: "tile_visibility",
            value: other.to_owned(),
        }),
    }
}

#[derive(Debug)]
pub enum SetupInstallError {
    InvalidInstallDir(PathBuf),
    InvalidRelativePath(PathBuf),
    MissingRequiredFile {
        file_id: &'static str,
        path: PathBuf,
    },
    InvalidProviderMode {
        field_name: &'static str,
        value: String,
    },
}

impl SetupInstallError {
    pub fn is_invalid_install_dir(&self) -> bool {
        matches!(self, Self::InvalidInstallDir(_))
    }

    pub fn is_missing_required_file(&self) -> bool {
        matches!(self, Self::MissingRequiredFile { .. })
    }
}

impl fmt::Display for SetupInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstallDir(path) => {
                write!(formatter, "invalid install directory: {}", path.display())
            }
            Self::InvalidRelativePath(path) => {
                write!(
                    formatter,
                    "invalid install relative path: {}",
                    path.display()
                )
            }
            Self::MissingRequiredFile { file_id, path } => write!(
                formatter,
                "required install file {file_id} does not exist: {}",
                path.display()
            ),
            Self::InvalidProviderMode { field_name, value } => {
                write!(formatter, "invalid provider mode {field_name}: {value}")
            }
        }
    }
}

impl std::error::Error for SetupInstallError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_plan_resolves_packaged_relative_paths_under_install_dir()
    -> Result<(), SetupInstallError> {
        let root = unique_temp_dir("plan");
        let install_dir = root.join("install");
        create_file(&install_dir.join("win_service.exe"));
        create_file(&install_dir.join(r"provider\windows_provider.dll"));
        create_file(&install_dir.join("selected_templates.json"));
        create_file(&install_dir.join(r"models\face_detection_yunet_2023mar.onnx"));
        create_file(&install_dir.join(r"models\face_recognition_sface_2021dec.onnx"));
        create_file(&install_dir.join(r"models\minifasnet_v2.onnx"));
        create_file(&install_dir.join(r"models\yolov8n.onnx"));

        let plan = build_install_plan(&InstallSystemComponentsPayload {
            install_dir: install_dir.clone(),
            ..InstallSystemComponentsPayload {
                install_dir: install_dir.clone(),
                configure_local_camera_auth: true,
                start_service: true,
                service_binary_relative_path: PathBuf::from("win_service.exe"),
                provider_binary_relative_path: PathBuf::from(r"provider\windows_provider.dll"),
                face_template_relative_path: PathBuf::from("selected_templates.json"),
                yunet_model_relative_path: PathBuf::from(
                    r"models\face_detection_yunet_2023mar.onnx",
                ),
                sface_model_relative_path: PathBuf::from(
                    r"models\face_recognition_sface_2021dec.onnx",
                ),
                minifasnet_model_relative_path: PathBuf::from(r"models\minifasnet_v2.onnx"),
                presence_person_model_relative_path: PathBuf::from(r"models\yolov8n.onnx"),
                presence_person_model_config_relative_path: PathBuf::new(),
                presence_pose_bridge_relative_path: PathBuf::from(
                    r"native\winfaceunlock_mediapipe_bridge.dll",
                ),
                presence_pose_model_relative_path: PathBuf::from(
                    r"models\pose_landmarker_lite.task",
                ),
                camera_id: "opencv-index:0".to_owned(),
                match_threshold: None,
                required_consecutive_match_count: None,
                provider_mode: Default::default(),
            }
        })?;

        assert_eq!(
            plan.service_plan.service_binary_path,
            install_dir.join("win_service.exe")
        );
        assert_eq!(
            plan.auth_config
                .as_ref()
                .map(|config| &config.yunet_model_path),
            Some(&install_dir.join(r"models\face_detection_yunet_2023mar.onnx"))
        );
        assert_eq!(
            plan.auth_config
                .as_ref()
                .map(|config| &config.presence_person_model_path),
            Some(&install_dir.join(r"models\yolov8n.onnx"))
        );
        assert_eq!(
            plan.auth_config
                .as_ref()
                .and_then(|config| config.presence_person_model_config_path.as_ref()),
            None
        );
        remove_temp_dir(&root);
        Ok(())
    }

    #[test]
    fn install_relative_path_rejects_parent_traversal() {
        let result = validate_install_relative_path(Path::new(r"..\win_service.exe"));

        assert!(matches!(
            result,
            Err(SetupInstallError::InvalidRelativePath(_))
        ));
    }

    #[test]
    fn invalid_provider_mode_is_rejected() {
        let result = validate_wake_auth_source("network-camera");

        assert!(matches!(
            result,
            Err(SetupInstallError::InvalidProviderMode { .. })
        ));
    }

    #[test]
    fn install_plan_without_local_camera_auth_does_not_require_face_template()
    -> Result<(), SetupInstallError> {
        let root = unique_temp_dir("components-only");
        let install_dir = root.join("install");
        create_file(&install_dir.join("win_service.exe"));
        create_file(&install_dir.join(r"provider\windows_provider.dll"));

        let plan = build_install_plan(&InstallSystemComponentsPayload {
            install_dir: install_dir.clone(),
            ..Default::default()
        })?;

        assert_eq!(plan.auth_config, None);
        assert_eq!(
            plan.provider_plan.provider_binary_path,
            install_dir.join(r"provider\windows_provider.dll")
        );
        remove_temp_dir(&root);
        Ok(())
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "winfaceunlock-install-{name}-{}",
            std::process::id()
        ))
    }

    fn create_file(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, b"payload");
    }

    fn remove_temp_dir(path: &Path) {
        if path.exists() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
