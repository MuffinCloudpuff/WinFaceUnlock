#![allow(unsafe_code)]

use std::fmt;

use common_protocol::{
    CredentialMaterialProtection, ProtectedCredentialMaterial, ProtocolError, UserId,
};

#[derive(Clone, Eq, PartialEq)]
pub struct CredentialMaterialSecret {
    pub user_id: UserId,
    pub domain: String,
    pub username: String,
    pub password: Vec<u8>,
}

impl fmt::Debug for CredentialMaterialSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialMaterialSecret")
            .field("user_id", &self.user_id)
            .field("domain", &self.domain)
            .field("username", &self.username)
            .field("password_len", &self.password.len())
            .finish()
    }
}

pub trait CredentialMaterialProtector {
    fn protect_credential_material(
        &self,
        credential_material: CredentialMaterialSecret,
    ) -> Result<ProtectedCredentialMaterial, ProtocolError>;

    fn unprotect_credential_material(
        &self,
        protected_credential_material: &ProtectedCredentialMaterial,
    ) -> Result<CredentialMaterialSecret, ProtocolError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DpapiLocalMachineCredentialMaterialProtector;

impl CredentialMaterialProtector for DpapiLocalMachineCredentialMaterialProtector {
    fn protect_credential_material(
        &self,
        credential_material: CredentialMaterialSecret,
    ) -> Result<ProtectedCredentialMaterial, ProtocolError> {
        Ok(ProtectedCredentialMaterial {
            user_id: credential_material.user_id,
            domain: credential_material.domain,
            username: credential_material.username,
            protected_password: dpapi::protect_local_machine(&credential_material.password)?,
            protection: CredentialMaterialProtection::DpapiLocalMachineV1,
        })
    }

    fn unprotect_credential_material(
        &self,
        protected_credential_material: &ProtectedCredentialMaterial,
    ) -> Result<CredentialMaterialSecret, ProtocolError> {
        if protected_credential_material.protection
            != CredentialMaterialProtection::DpapiLocalMachineV1
        {
            return Err(ProtocolError::InvalidMessage);
        }

        Ok(CredentialMaterialSecret {
            user_id: protected_credential_material.user_id.clone(),
            domain: protected_credential_material.domain.clone(),
            username: protected_credential_material.username.clone(),
            password: dpapi::unprotect_local_machine(
                &protected_credential_material.protected_password,
            )?,
        })
    }
}

#[cfg(windows)]
mod dpapi {
    use std::ptr;

    use common_protocol::ProtocolError;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE, CryptProtectData, CryptUnprotectData,
        },
    };

    pub fn protect_local_machine(bytes: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
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

        blob_to_vec(protect_api_call_succeeded, output)
    }

    pub fn unprotect_local_machine(bytes: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr().cast_mut(),
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

        blob_to_vec(unprotect_api_call_succeeded, output)
    }

    fn blob_to_vec(
        api_call_succeeded: i32,
        output: CRYPT_INTEGER_BLOB,
    ) -> Result<Vec<u8>, ProtocolError> {
        if api_call_succeeded == 0 || output.pbData.is_null() || output.cbData == 0 {
            return Err(ProtocolError::Unauthorized);
        }

        let bytes =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(output.pbData.cast());
        }
        Ok(bytes)
    }
}

#[cfg(not(windows))]
mod dpapi {
    use common_protocol::ProtocolError;

    pub fn protect_local_machine(_bytes: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        Err(ProtocolError::TransportUnavailable)
    }

    pub fn unprotect_local_machine(_bytes: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        Err(ProtocolError::TransportUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use common_protocol::UserId;

    use super::*;

    #[cfg(windows)]
    #[test]
    fn dpapi_local_machine_credential_material_round_trips() -> Result<(), ProtocolError> {
        let protector = DpapiLocalMachineCredentialMaterialProtector;
        let secret = CredentialMaterialSecret {
            user_id: UserId("user-1".to_owned()),
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: b"correct horse battery".to_vec(),
        };

        let protected = protector.protect_credential_material(secret.clone())?;
        let restored = protector.unprotect_credential_material(&protected)?;

        assert_eq!(restored, secret);
        assert_ne!(protected.protected_password, b"correct horse battery");
        Ok(())
    }

    #[test]
    fn credential_material_secret_debug_redacts_password_bytes() {
        let secret = CredentialMaterialSecret {
            user_id: UserId("user-1".to_owned()),
            domain: ".".to_owned(),
            username: "leo16".to_owned(),
            password: b"secret".to_vec(),
        };

        let debug = format!("{secret:?}");

        assert!(debug.contains("password_len"));
        assert!(!debug.contains("secret"));
    }
}
