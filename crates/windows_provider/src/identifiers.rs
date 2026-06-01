use common_protocol::PROVIDER_NAME;
use windows_core::GUID;

pub const WINDOWS_PROVIDER_NAME: &str = PROVIDER_NAME;

// Project-owned CLSID. Do not reuse sample provider GUIDs from reference projects.
pub const PROVIDER_CLSID: GUID = GUID::from_u128(0x019e7c17_2ba4_74f1_879d_025113ecfd98);
pub const PROVIDER_CLSID_BRACED: &str = "{019E7C17-2BA4-74F1-879D-025113ECFD98}";

pub const PROVIDER_ROOT_REGISTRY_PATH: &str = r"SOFTWARE\WinFaceUnlock\CredentialProvider";
pub const PROVIDER_CLSID_REGISTRY_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{019E7C17-2BA4-74F1-879D-025113ECFD98}";
pub const COM_CLSID_REGISTRY_PATH: &str =
    r"SOFTWARE\Classes\CLSID\{019E7C17-2BA4-74F1-879D-025113ECFD98}";
pub const COM_INPROC_SERVER_REGISTRY_PATH: &str =
    r"SOFTWARE\Classes\CLSID\{019E7C17-2BA4-74F1-879D-025113ECFD98}\InprocServer32";
