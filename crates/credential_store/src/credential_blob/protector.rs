use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use common_protocol::{CredentialRef, UserId};

use crate::{
    CredentialStoreError, MasterKey, SecureRandom,
    credential_blob::{
        format::{CredentialBlob, CredentialBlobAlgorithm},
        secret::CredentialSecret,
    },
};

const AES_GCM_NONCE_LEN: usize = 12;
const CREDENTIAL_BLOB_AAD_CONTEXT: &[u8] = b"WinFaceUnlockCredentialBlobAadV1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBlobAssociatedData {
    pub user_id: UserId,
    pub credential_ref: CredentialRef,
}

impl CredentialBlobAssociatedData {
    fn to_aad_bytes(&self) -> Vec<u8> {
        let mut aad = Vec::new();
        aad.extend_from_slice(CREDENTIAL_BLOB_AAD_CONTEXT);
        append_len_prefixed_bytes(&mut aad, self.user_id.0.as_bytes());
        append_len_prefixed_bytes(&mut aad, self.credential_ref.0.as_bytes());
        aad
    }
}

pub trait CredentialBlobProtector {
    fn encrypt_credential_secret(
        &self,
        master_key: &MasterKey,
        associated_data: &CredentialBlobAssociatedData,
        credential_secret: &CredentialSecret,
    ) -> Result<CredentialBlob, CredentialStoreError>;

    fn decrypt_credential_secret(
        &self,
        master_key: &MasterKey,
        associated_data: &CredentialBlobAssociatedData,
        credential_blob: &CredentialBlob,
    ) -> Result<CredentialSecret, CredentialStoreError>;
}

pub struct AesGcmCredentialBlobProtector<R: SecureRandom> {
    secure_random: R,
}

impl<R: SecureRandom> AesGcmCredentialBlobProtector<R> {
    pub fn new(secure_random: R) -> Self {
        Self { secure_random }
    }
}

impl<R: SecureRandom> CredentialBlobProtector for AesGcmCredentialBlobProtector<R> {
    fn encrypt_credential_secret(
        &self,
        master_key: &MasterKey,
        associated_data: &CredentialBlobAssociatedData,
        credential_secret: &CredentialSecret,
    ) -> Result<CredentialBlob, CredentialStoreError> {
        let cipher = cipher_from_master_key(master_key)?;
        let mut nonce = [0_u8; AES_GCM_NONCE_LEN];
        self.secure_random.fill_cryptographic_bytes(&mut nonce)?;
        let aad = associated_data.to_aad_bytes();
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: credential_secret.expose_for_encryption(),
                    aad: &aad,
                },
            )
            .map_err(|_| CredentialStoreError::CredentialBlobEncryptionFailed)?;

        Ok(CredentialBlob::new(
            CredentialBlobAlgorithm::Aes256GcmV1,
            nonce.to_vec(),
            ciphertext,
        ))
    }

    fn decrypt_credential_secret(
        &self,
        master_key: &MasterKey,
        associated_data: &CredentialBlobAssociatedData,
        credential_blob: &CredentialBlob,
    ) -> Result<CredentialSecret, CredentialStoreError> {
        if credential_blob.algorithm != CredentialBlobAlgorithm::Aes256GcmV1
            || credential_blob.nonce.len() != AES_GCM_NONCE_LEN
        {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        let cipher = cipher_from_master_key(master_key)?;
        let aad = associated_data.to_aad_bytes();
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&credential_blob.nonce),
                Payload {
                    msg: &credential_blob.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CredentialStoreError::CredentialBlobDecryptionFailed)?;

        Ok(CredentialSecret::from_secret_bytes(plaintext))
    }
}

fn cipher_from_master_key(master_key: &MasterKey) -> Result<Aes256Gcm, CredentialStoreError> {
    Aes256Gcm::new_from_slice(master_key.expose_for_cryptographic_use())
        .map_err(|_| CredentialStoreError::CredentialBlobEncryptionFailed)
}

fn append_len_prefixed_bytes(destination: &mut Vec<u8>, bytes: &[u8]) {
    destination.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    destination.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use common_protocol::{CredentialRef, UserId};

    use crate::{
        CredentialBlobProtector, CredentialSecret, CredentialStoreError, MASTER_KEY_LEN, MasterKey,
        credential_blob::protector::{AesGcmCredentialBlobProtector, CredentialBlobAssociatedData},
        secure_random::tests::FixedSecureRandom,
    };

    #[test]
    fn credential_secret_encrypts_and_decrypts() -> Result<(), CredentialStoreError> {
        let protector = AesGcmCredentialBlobProtector::new(FixedSecureRandom::new(5));
        let master_key = MasterKey::from_bytes([8_u8; MASTER_KEY_LEN]);
        let associated_data = associated_data("user-1", "cred-1");
        let credential_secret = CredentialSecret::from_utf8_password("correct horse battery");

        let blob = protector.encrypt_credential_secret(
            &master_key,
            &associated_data,
            &credential_secret,
        )?;
        let decrypted =
            protector.decrypt_credential_secret(&master_key, &associated_data, &blob)?;

        assert_eq!(decrypted.expose_for_encryption(), b"correct horse battery");
        assert!(
            !blob
                .serialize()?
                .windows(21)
                .any(|w| w == b"correct horse battery")
        );
        Ok(())
    }

    #[test]
    fn credential_secret_rejects_wrong_associated_data() -> Result<(), CredentialStoreError> {
        let protector = AesGcmCredentialBlobProtector::new(FixedSecureRandom::new(5));
        let master_key = MasterKey::from_bytes([8_u8; MASTER_KEY_LEN]);
        let credential_secret = CredentialSecret::from_utf8_password("password");
        let blob = protector.encrypt_credential_secret(
            &master_key,
            &associated_data("user-1", "cred-1"),
            &credential_secret,
        )?;

        let decrypt_result = protector.decrypt_credential_secret(
            &master_key,
            &associated_data("user-2", "cred-1"),
            &blob,
        );

        assert_eq!(
            decrypt_result,
            Err(CredentialStoreError::CredentialBlobDecryptionFailed)
        );
        Ok(())
    }

    fn associated_data(user_id: &str, credential_ref: &str) -> CredentialBlobAssociatedData {
        CredentialBlobAssociatedData {
            user_id: UserId(user_id.to_owned()),
            credential_ref: CredentialRef(credential_ref.to_owned()),
        }
    }
}
