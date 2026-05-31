use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::CredentialStoreError;

const PROTECTED_MASTER_KEY_VERSION: u8 = 1;
const PROTECTED_MASTER_KEY_MAGIC: &[u8; 8] = b"WFUKEY01";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MasterKeyHandle {
    pub protected_key_path: String,
}

pub struct ProtectedMasterKeyFile {
    path: PathBuf,
}

impl ProtectedMasterKeyFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self, protected_blob: &[u8]) -> Result<MasterKeyHandle, CredentialStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&self.path)?;
        file.write_all(PROTECTED_MASTER_KEY_MAGIC)?;
        file.write_all(&[PROTECTED_MASTER_KEY_VERSION])?;
        file.write_all(&(protected_blob.len() as u32).to_le_bytes())?;
        file.write_all(protected_blob)?;
        file.flush()?;

        Ok(MasterKeyHandle {
            protected_key_path: self.path.display().to_string(),
        })
    }

    pub fn load(&self) -> Result<Vec<u8>, CredentialStoreError> {
        let mut file = fs::File::open(&self.path)?;
        let mut magic = [0_u8; 8];
        read_exact_or_invalid_key_file(&mut file, &mut magic)?;
        if &magic != PROTECTED_MASTER_KEY_MAGIC {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }

        let mut version = [0_u8; 1];
        read_exact_or_invalid_key_file(&mut file, &mut version)?;
        if version[0] != PROTECTED_MASTER_KEY_VERSION {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }

        let mut len = [0_u8; 4];
        read_exact_or_invalid_key_file(&mut file, &mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len == 0 {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }

        let mut protected = vec![0_u8; len];
        read_exact_or_invalid_key_file(&mut file, &mut protected)?;
        Ok(protected)
    }
}

fn read_exact_or_invalid_key_file(
    reader: &mut impl Read,
    destination: &mut [u8],
) -> Result<(), CredentialStoreError> {
    reader
        .read_exact(destination)
        .map_err(|_| CredentialStoreError::InvalidProtectedKeyFile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn protected_master_key_file_round_trips_blob() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("protected-master-key.bin")?;
        let key_file = ProtectedMasterKeyFile::new(&path);
        let protected_blob = vec![1_u8, 2, 3, 4];

        key_file.save(&protected_blob)?;
        let loaded = key_file.load()?;
        let _ = fs::remove_file(path);

        assert_eq!(loaded, protected_blob);
        Ok(())
    }

    #[test]
    fn invalid_magic_is_rejected() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("invalid-master-key.bin")?;
        fs::write(&path, b"bad")?;
        let key_file = ProtectedMasterKeyFile::new(&path);

        let result = key_file.load();
        let _ = fs::remove_file(path);

        assert_eq!(result, Err(CredentialStoreError::InvalidProtectedKeyFile));
        Ok(())
    }

    fn unique_temp_path(name: &str) -> Result<PathBuf, CredentialStoreError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CredentialStoreError::IoFailed)?
            .as_nanos();
        Ok(std::env::temp_dir().join(format!(
            "winfaceunlock-{}-{}-{name}",
            std::process::id(),
            nanos
        )))
    }
}
