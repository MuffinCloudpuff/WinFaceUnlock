use crate::CredentialStoreError;

pub trait SecureRandom {
    fn fill_cryptographic_bytes(&self, destination: &mut [u8]) -> Result<(), CredentialStoreError>;
}

#[cfg(windows)]
pub struct WindowsSecureRandom;

#[cfg(windows)]
impl WindowsSecureRandom {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Default for WindowsSecureRandom {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
impl SecureRandom for WindowsSecureRandom {
    fn fill_cryptographic_bytes(&self, destination: &mut [u8]) -> Result<(), CredentialStoreError> {
        use std::ptr;

        use windows_sys::Win32::Security::Cryptography::{
            BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
        };

        let random_api_status = unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                destination.as_mut_ptr(),
                destination.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };

        if random_api_status != 0 {
            return Err(CredentialStoreError::RandomGenerationFailed);
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::CredentialStoreError;

    use super::SecureRandom;

    pub(crate) struct FixedSecureRandom {
        byte: u8,
    }

    impl FixedSecureRandom {
        pub(crate) fn new(byte: u8) -> Self {
            Self { byte }
        }
    }

    impl SecureRandom for FixedSecureRandom {
        fn fill_cryptographic_bytes(
            &self,
            destination: &mut [u8],
        ) -> Result<(), CredentialStoreError> {
            destination.fill(self.byte);
            Ok(())
        }
    }

    #[test]
    fn fixed_secure_random_fills_destination() -> Result<(), CredentialStoreError> {
        let random = FixedSecureRandom::new(9);
        let mut bytes = [0_u8; 4];

        random.fill_cryptographic_bytes(&mut bytes)?;

        assert_eq!(bytes, [9, 9, 9, 9]);
        Ok(())
    }
}
