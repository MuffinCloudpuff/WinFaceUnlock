use crate::CredentialStoreError;

const CREDENTIAL_BLOB_MAGIC: &[u8; 8] = b"WFUCRD01";
const CREDENTIAL_BLOB_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialBlobAlgorithm {
    Aes256GcmV1,
}

impl CredentialBlobAlgorithm {
    fn to_format_id(self) -> u8 {
        match self {
            Self::Aes256GcmV1 => 1,
        }
    }

    fn from_format_id(format_id: u8) -> Result<Self, CredentialStoreError> {
        match format_id {
            1 => Ok(Self::Aes256GcmV1),
            _ => Err(CredentialStoreError::InvalidCredentialBlob),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBlob {
    pub algorithm: CredentialBlobAlgorithm,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl CredentialBlob {
    pub fn new(algorithm: CredentialBlobAlgorithm, nonce: Vec<u8>, ciphertext: Vec<u8>) -> Self {
        Self {
            algorithm,
            nonce,
            ciphertext,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, CredentialStoreError> {
        if self.nonce.is_empty() || self.ciphertext.is_empty() {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        if self.nonce.len() > u8::MAX as usize || self.ciphertext.len() > u32::MAX as usize {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        let mut serialized =
            Vec::with_capacity(8 + 1 + 1 + 1 + 4 + self.nonce.len() + self.ciphertext.len());
        serialized.extend_from_slice(CREDENTIAL_BLOB_MAGIC);
        serialized.push(CREDENTIAL_BLOB_VERSION);
        serialized.push(self.algorithm.to_format_id());
        serialized.push(self.nonce.len() as u8);
        serialized.extend_from_slice(&(self.ciphertext.len() as u32).to_le_bytes());
        serialized.extend_from_slice(&self.nonce);
        serialized.extend_from_slice(&self.ciphertext);
        Ok(serialized)
    }

    pub fn deserialize(serialized: &[u8]) -> Result<Self, CredentialStoreError> {
        const HEADER_LEN: usize = 8 + 1 + 1 + 1 + 4;
        if serialized.len() < HEADER_LEN {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        if &serialized[0..8] != CREDENTIAL_BLOB_MAGIC {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        if serialized[8] != CREDENTIAL_BLOB_VERSION {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        let algorithm = CredentialBlobAlgorithm::from_format_id(serialized[9])?;
        let nonce_len = serialized[10] as usize;
        let ciphertext_len = u32::from_le_bytes([
            serialized[11],
            serialized[12],
            serialized[13],
            serialized[14],
        ]) as usize;

        if nonce_len == 0 || ciphertext_len == 0 {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        let expected_len = HEADER_LEN
            .checked_add(nonce_len)
            .and_then(|len| len.checked_add(ciphertext_len))
            .ok_or(CredentialStoreError::InvalidCredentialBlob)?;
        if serialized.len() != expected_len {
            return Err(CredentialStoreError::InvalidCredentialBlob);
        }

        let nonce_start = HEADER_LEN;
        let nonce_end = nonce_start + nonce_len;
        let ciphertext_end = nonce_end + ciphertext_len;

        Ok(Self {
            algorithm,
            nonce: serialized[nonce_start..nonce_end].to_vec(),
            ciphertext: serialized[nonce_end..ciphertext_end].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_blob_round_trips_serialized_format() -> Result<(), CredentialStoreError> {
        let blob = CredentialBlob::new(
            CredentialBlobAlgorithm::Aes256GcmV1,
            vec![1; 12],
            vec![2; 16],
        );

        let serialized = blob.serialize()?;
        let deserialized = CredentialBlob::deserialize(&serialized)?;

        assert_eq!(deserialized, blob);
        Ok(())
    }

    #[test]
    fn credential_blob_rejects_wrong_magic() {
        let invalid = b"BADCRD01\x01\x01\x0c\x10\x00\x00\x00";

        assert_eq!(
            CredentialBlob::deserialize(invalid),
            Err(CredentialStoreError::InvalidCredentialBlob)
        );
    }
}
