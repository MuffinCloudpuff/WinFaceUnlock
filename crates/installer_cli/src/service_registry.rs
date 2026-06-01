#![allow(unsafe_code)]

use std::{fmt, path::PathBuf};

const SERVICE_CONFIG_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\Service";

const REG_AUTH_MODE: &str = "AuthMode";
const REG_FACE_TEMPLATE_PATH: &str = "FaceTemplatePath";
const REG_CAMERA_ID: &str = "CameraId";
const REG_YUNET_MODEL_PATH: &str = "YuNetModelPath";
const REG_SFACE_MODEL_PATH: &str = "SFaceModelPath";
const REG_FRAME_WIDTH: &str = "FrameWidth";
const REG_FRAME_HEIGHT: &str = "FrameHeight";
const REG_MAX_AUTH_FRAMES: &str = "MaxAuthFrames";
const REG_REQUIRED_CONSECUTIVE: &str = "RequiredConsecutiveMatchCount";
const REG_MATCH_THRESHOLD: &str = "MatchThreshold";

const AUTH_MODE_LOCAL_CAMERA: &str = "local-camera";

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceAuthRegistryConfig {
    pub auth_mode: String,
    pub face_template_path: PathBuf,
    pub camera_id: String,
    pub yunet_model_path: PathBuf,
    pub sface_model_path: PathBuf,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub max_auth_frames: u32,
    pub required_consecutive_match_count: u32,
    pub match_threshold: f32,
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
            frame_width: None,
            frame_height: None,
            max_auth_frames: 30,
            required_consecutive_match_count: 2,
            match_threshold: 0.55,
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
        }
    }
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
        assert_eq!(config.match_threshold, 0.55);
    }
}
