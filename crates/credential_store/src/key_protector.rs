use crate::{CredentialStoreError, MasterKey};

pub trait KeyProtector {
    fn generate_master_key(&self) -> Result<MasterKey, CredentialStoreError>;
    fn protect_master_key(&self, key: &MasterKey) -> Result<Vec<u8>, CredentialStoreError>;
    fn unprotect_master_key(&self, protected: &[u8]) -> Result<MasterKey, CredentialStoreError>;
}

#[cfg(windows)]
pub struct WindowsDpapiKeyProtector;

#[cfg(windows)]
impl WindowsDpapiKeyProtector {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Default for WindowsDpapiKeyProtector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl KeyProtector for WindowsDpapiKeyProtector {
    fn generate_master_key(&self) -> Result<MasterKey, CredentialStoreError> {
        windows_dpapi::generate_master_key()
    }

    fn protect_master_key(&self, key: &MasterKey) -> Result<Vec<u8>, CredentialStoreError> {
        windows_dpapi::protect_master_key(key)
    }

    fn unprotect_master_key(&self, protected: &[u8]) -> Result<MasterKey, CredentialStoreError> {
        windows_dpapi::unprotect_master_key(protected)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_dpapi {
    use std::ptr;

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CryptProtectData, CryptUnprotectData,
        },
    };

    use crate::{
        CredentialStoreError, MASTER_KEY_LEN, MasterKey, SecureRandom, WindowsSecureRandom,
    };

    pub fn generate_master_key() -> Result<MasterKey, CredentialStoreError> {
        let mut bytes = [0_u8; MASTER_KEY_LEN];
        WindowsSecureRandom::new()
            .fill_cryptographic_bytes(&mut bytes)
            .map_err(|_| CredentialStoreError::KeyGenerationFailed)?;
        Ok(MasterKey::from_bytes(bytes))
    }

    pub fn protect_master_key(key: &MasterKey) -> Result<Vec<u8>, CredentialStoreError> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: MASTER_KEY_LEN as u32,
            pbData: key.expose_for_cryptographic_use().as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        let protect_api_call_succeeded = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut output,
            )
        };

        blob_to_vec(
            protect_api_call_succeeded,
            output,
            CredentialStoreError::KeyProtectionFailed,
        )
    }

    pub fn unprotect_master_key(protected: &[u8]) -> Result<MasterKey, CredentialStoreError> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
            pbData: protected.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        let unprotect_api_call_succeeded = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        };

        let bytes = blob_to_vec(
            unprotect_api_call_succeeded,
            output,
            CredentialStoreError::KeyUnprotectFailed,
        )?;
        let key_bytes: [u8; MASTER_KEY_LEN] = bytes
            .try_into()
            .map_err(|_| CredentialStoreError::KeyUnprotectFailed)?;
        Ok(MasterKey::from_bytes(key_bytes))
    }

    fn blob_to_vec(
        api_call_succeeded: i32,
        output: CRYPT_INTEGER_BLOB,
        err: CredentialStoreError,
    ) -> Result<Vec<u8>, CredentialStoreError> {
        if api_call_succeeded == 0 || output.pbData.is_null() || output.cbData == 0 {
            return Err(err);
        }

        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(output.pbData.cast());
        }
        Ok(bytes)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{CredentialStoreError, MASTER_KEY_LEN, MasterKey};

    use super::KeyProtector;

    #[derive(Default)]
    pub(crate) struct MemoryKeyProtector;

    impl KeyProtector for MemoryKeyProtector {
        fn generate_master_key(&self) -> Result<MasterKey, CredentialStoreError> {
            Ok(MasterKey::from_bytes([7_u8; MASTER_KEY_LEN]))
        }

        fn protect_master_key(&self, key: &MasterKey) -> Result<Vec<u8>, CredentialStoreError> {
            Ok(key
                .expose_for_cryptographic_use()
                .iter()
                .map(|b| b ^ 0xA5)
                .collect())
        }

        fn unprotect_master_key(
            &self,
            protected: &[u8],
        ) -> Result<MasterKey, CredentialStoreError> {
            let bytes: Vec<u8> = protected.iter().map(|b| b ^ 0xA5).collect();
            let bytes: [u8; MASTER_KEY_LEN] = bytes
                .try_into()
                .map_err(|_| CredentialStoreError::KeyUnprotectFailed)?;
            Ok(MasterKey::from_bytes(bytes))
        }
    }

    #[test]
    fn memory_key_protector_round_trips_key() -> Result<(), CredentialStoreError> {
        let protector = MemoryKeyProtector;
        let key = protector.generate_master_key()?;
        let protected = protector.protect_master_key(&key)?;
        let restored = protector.unprotect_master_key(&protected)?;

        assert_eq!(
            key.expose_for_cryptographic_use(),
            restored.expose_for_cryptographic_use()
        );
        assert_ne!(protected, key.expose_for_cryptographic_use());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn windows_dpapi_protects_and_unprotects_master_key() -> Result<(), CredentialStoreError> {
        let protector = super::WindowsDpapiKeyProtector::new();
        let master_key = protector.generate_master_key()?;
        let protected = protector.protect_master_key(&master_key)?;
        let restored = protector.unprotect_master_key(&protected)?;

        assert_eq!(
            master_key.expose_for_cryptographic_use(),
            restored.expose_for_cryptographic_use()
        );
        assert_ne!(protected, master_key.expose_for_cryptographic_use());
        Ok(())
    }
}
