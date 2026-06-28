#![allow(unsafe_code)]

mod auth_package;
mod broker_client;
mod class_factory;
mod credential;
mod dll_exports;
mod dll_lifetime;
mod fields;
mod identifiers;
mod provider;
mod provider_config;
mod provider_log;
mod provider_state;
mod serialization;

pub use auth_package::retrieve_negotiate_auth_package_id;
pub use identifiers::{
    COM_CLSID_REGISTRY_PATH, COM_INPROC_SERVER_REGISTRY_PATH, PROVIDER_CLSID,
    PROVIDER_CLSID_BRACED, PROVIDER_CLSID_REGISTRY_PATH, PROVIDER_ROOT_REGISTRY_PATH,
    WINDOWS_PROVIDER_NAME,
};
pub use provider_config::{
    LOGON_WAKE_MODE_BACKGROUND_POLICY, LOGON_WAKE_MODE_BACKGROUND_SILENT_RECOGNITION,
    LOGON_WAKE_MODE_HYBRID, LOGON_WAKE_MODE_INPUT_TRIGGERED, LOGON_WAKE_MODE_TRIGGERED_RECOGNITION,
    REG_VALUE_AUTO_WAKE_ON_ADVISE, REG_VALUE_LOGON_WAKE_MODE, REG_VALUE_TILE_VISIBILITY,
    REG_VALUE_WAKE_AUTH_SOURCE, TILE_VISIBILITY_HIDDEN_UNTIL_READY, TILE_VISIBILITY_VISIBLE,
    WAKE_AUTH_SOURCE_LOCAL_CAMERA, WAKE_AUTH_SOURCE_MANUAL_TEST,
};

#[cfg(windows)]
pub use dll_exports::{DllCanUnloadNow, DllGetClassObject, DllMain};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_uses_project_name() {
        assert_eq!(WINDOWS_PROVIDER_NAME, "WinFaceUnlockProvider");
    }

    #[test]
    fn provider_clsid_is_braced_for_registry_paths() {
        assert!(PROVIDER_CLSID_BRACED.starts_with('{'));
        assert!(PROVIDER_CLSID_BRACED.ends_with('}'));
        assert!(PROVIDER_CLSID_REGISTRY_PATH.contains(PROVIDER_CLSID_BRACED));
    }
}
