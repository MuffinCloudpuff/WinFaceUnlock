use common_protocol::AuthSource;

use crate::{identifiers::PROVIDER_ROOT_REGISTRY_PATH, provider_state::ProviderTileVisibility};

pub const REG_VALUE_TILE_VISIBILITY: &str = "TileVisibility";
pub const REG_VALUE_AUTO_WAKE_ON_ADVISE: &str = "AutoWakeOnAdvise";
pub const REG_VALUE_WAKE_AUTH_SOURCE: &str = "WakeAuthSource";
pub const REG_VALUE_LOGON_WAKE_MODE: &str = "LogonWakeMode";
pub const TILE_VISIBILITY_VISIBLE: &str = "visible";
pub const TILE_VISIBILITY_HIDDEN_UNTIL_READY: &str = "hidden-until-ready";
pub const WAKE_AUTH_SOURCE_LOCAL_CAMERA: &str = "local-camera";
pub const WAKE_AUTH_SOURCE_MANUAL_TEST: &str = "manual-test";
pub const LOGON_WAKE_MODE_INPUT_TRIGGERED: &str = "input-triggered";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderRuntimeConfig {
    pub tile_visibility: ProviderTileVisibility,
    pub auto_wake_on_advise: bool,
    pub wake_auth_source: AuthSource,
    pub logon_wake_mode: Option<ProviderLogonWakeMode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderLogonWakeMode {
    InputTriggered,
}

impl ProviderRuntimeConfig {
    pub fn from_registry_or_default() -> Self {
        let auto_wake_on_advise =
            registry::read_string_value(PROVIDER_ROOT_REGISTRY_PATH, REG_VALUE_AUTO_WAKE_ON_ADVISE)
                .as_deref()
                .map(bool_from_config_value)
                .unwrap_or(false);
        Self {
            tile_visibility: registry::read_string_value(
                PROVIDER_ROOT_REGISTRY_PATH,
                REG_VALUE_TILE_VISIBILITY,
            )
            .as_deref()
            .map(tile_visibility_from_config_value)
            .unwrap_or_default(),
            auto_wake_on_advise,
            wake_auth_source: registry::read_string_value(
                PROVIDER_ROOT_REGISTRY_PATH,
                REG_VALUE_WAKE_AUTH_SOURCE,
            )
            .as_deref()
            .map(wake_auth_source_from_config_value)
            .unwrap_or(AuthSource::LocalCamera),
            logon_wake_mode: registry::read_string_value(
                PROVIDER_ROOT_REGISTRY_PATH,
                REG_VALUE_LOGON_WAKE_MODE,
            )
            .as_deref()
            .and_then(logon_wake_mode_from_config_value)
            .or_else(|| legacy_logon_wake_mode(auto_wake_on_advise)),
        }
    }
}

impl Default for ProviderRuntimeConfig {
    fn default() -> Self {
        Self {
            tile_visibility: ProviderTileVisibility::default(),
            auto_wake_on_advise: false,
            wake_auth_source: AuthSource::LocalCamera,
            logon_wake_mode: None,
        }
    }
}

fn tile_visibility_from_config_value(value: &str) -> ProviderTileVisibility {
    match value {
        TILE_VISIBILITY_HIDDEN_UNTIL_READY => ProviderTileVisibility::HiddenUntilCredentialReady,
        TILE_VISIBILITY_VISIBLE => ProviderTileVisibility::VisibleBeforeCredentialReady,
        _ => ProviderTileVisibility::VisibleBeforeCredentialReady,
    }
}

fn bool_from_config_value(value: &str) -> bool {
    matches!(value, "1" | "true" | "TRUE" | "True" | "yes" | "YES")
}

fn wake_auth_source_from_config_value(value: &str) -> AuthSource {
    match value {
        WAKE_AUTH_SOURCE_MANUAL_TEST => AuthSource::ManualTest,
        WAKE_AUTH_SOURCE_LOCAL_CAMERA => AuthSource::LocalCamera,
        _ => AuthSource::LocalCamera,
    }
}

fn logon_wake_mode_from_config_value(value: &str) -> Option<ProviderLogonWakeMode> {
    match value {
        LOGON_WAKE_MODE_INPUT_TRIGGERED | "input_triggered" => {
            Some(ProviderLogonWakeMode::InputTriggered)
        }
        _ => None,
    }
}

fn legacy_logon_wake_mode(auto_wake_on_advise: bool) -> Option<ProviderLogonWakeMode> {
    if auto_wake_on_advise {
        Some(ProviderLogonWakeMode::InputTriggered)
    } else {
        None
    }
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegOpenKeyExW,
            RegQueryValueExW,
        },
    };

    use crate::provider_config::to_wide_null;

    pub fn read_string_value(path: &str, value_name: &str) -> Option<String> {
        let path_wide = to_wide_null(path);
        let mut key: HKEY = std::ptr::null_mut();
        let open_status = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path_wide.as_ptr(),
                0,
                KEY_READ,
                &mut key,
            )
        };
        if open_status != ERROR_SUCCESS {
            return None;
        }

        let value = query_string_value(key, value_name);
        unsafe {
            let _ = RegCloseKey(key);
        }
        value
    }

    fn query_string_value(key: HKEY, value_name: &str) -> Option<String> {
        let value_name_wide = to_wide_null(value_name);
        let mut value_type = 0_u32;
        let mut value_len_bytes = 0_u32;
        let probe_status = unsafe {
            RegQueryValueExW(
                key,
                value_name_wide.as_ptr(),
                std::ptr::null_mut(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut value_len_bytes,
            )
        };
        if probe_status != ERROR_SUCCESS || value_type != REG_SZ || value_len_bytes < 2 {
            return None;
        }

        let mut buffer = vec![0_u16; (value_len_bytes as usize).div_ceil(size_of::<u16>())];
        let read_status = unsafe {
            RegQueryValueExW(
                key,
                value_name_wide.as_ptr(),
                std::ptr::null_mut(),
                &mut value_type,
                buffer.as_mut_ptr().cast::<u8>(),
                &mut value_len_bytes,
            )
        };
        if read_status != ERROR_SUCCESS || value_type != REG_SZ {
            return None;
        }

        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end]).ok()
    }
}

#[cfg(not(windows))]
mod registry {
    pub fn read_string_value(_path: &str, _value_name: &str) -> Option<String> {
        None
    }
}

#[cfg(windows)]
fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_keeps_visible_tile_and_requires_explicit_auto_wake() {
        let config = ProviderRuntimeConfig::default();

        assert_eq!(
            config.tile_visibility,
            ProviderTileVisibility::VisibleBeforeCredentialReady
        );
        assert!(!config.auto_wake_on_advise);
        assert_eq!(config.wake_auth_source, AuthSource::LocalCamera);
        assert_eq!(config.logon_wake_mode, None);
    }

    #[test]
    fn tile_visibility_config_uses_explicit_values() {
        assert_eq!(
            tile_visibility_from_config_value(TILE_VISIBILITY_HIDDEN_UNTIL_READY),
            ProviderTileVisibility::HiddenUntilCredentialReady
        );
        assert_eq!(
            tile_visibility_from_config_value(TILE_VISIBILITY_VISIBLE),
            ProviderTileVisibility::VisibleBeforeCredentialReady
        );
    }

    #[test]
    fn boolean_config_rejects_ambiguous_values() {
        assert!(bool_from_config_value("true"));
        assert!(bool_from_config_value("1"));
        assert!(!bool_from_config_value("maybe"));
    }

    #[test]
    fn wake_auth_source_config_uses_explicit_values() {
        assert_eq!(
            wake_auth_source_from_config_value(WAKE_AUTH_SOURCE_MANUAL_TEST),
            AuthSource::ManualTest
        );
        assert_eq!(
            wake_auth_source_from_config_value(WAKE_AUTH_SOURCE_LOCAL_CAMERA),
            AuthSource::LocalCamera
        );
        assert_eq!(
            wake_auth_source_from_config_value("ambiguous"),
            AuthSource::LocalCamera
        );
    }

    #[test]
    fn logon_wake_mode_config_uses_backend_semantic_value() {
        assert_eq!(
            logon_wake_mode_from_config_value(LOGON_WAKE_MODE_INPUT_TRIGGERED),
            Some(ProviderLogonWakeMode::InputTriggered)
        );
        assert_eq!(
            logon_wake_mode_from_config_value("input_triggered"),
            Some(ProviderLogonWakeMode::InputTriggered)
        );
        assert_eq!(logon_wake_mode_from_config_value("keyboard"), None);
    }

    #[test]
    fn legacy_auto_wake_maps_to_input_triggered_logon_wake_mode() {
        assert_eq!(
            legacy_logon_wake_mode(true),
            Some(ProviderLogonWakeMode::InputTriggered)
        );
        assert_eq!(legacy_logon_wake_mode(false), None);
    }
}
