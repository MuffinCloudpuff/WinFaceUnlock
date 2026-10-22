use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::CredentialStoreError;

mod atomic_write;

const PROTECTED_MASTER_KEY_VERSION: u8 = 1;
const PROTECTED_MASTER_KEY_MAGIC: &[u8; 8] = b"WFUKEY01";
const MAX_PROTECTED_MASTER_KEY_LEN: usize = 64 * 1024;

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
        if !self.save_if_absent(protected_blob)? {
            return Err(CredentialStoreError::IoFailed);
        }
        Ok(MasterKeyHandle {
            protected_key_path: self.path.display().to_string(),
        })
    }

    /// Publishes a complete key file without ever replacing an existing key.
    /// `false` means another initializer already published the key to load.
    pub fn save_if_absent(&self, protected_blob: &[u8]) -> Result<bool, CredentialStoreError> {
        if protected_blob.is_empty() || protected_blob.len() > MAX_PROTECTED_MASTER_KEY_LEN {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }
        atomic_write::publish_if_absent(&self.path, |file| {
            file.write_all(PROTECTED_MASTER_KEY_MAGIC)?;
            file.write_all(&[PROTECTED_MASTER_KEY_VERSION])?;
            file.write_all(&(protected_blob.len() as u32).to_le_bytes())?;
            file.write_all(protected_blob)
        })
        .map_err(Into::into)
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
        if len == 0 || len > MAX_PROTECTED_MASTER_KEY_LEN {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }

        let mut protected = vec![0_u8; len];
        read_exact_or_invalid_key_file(&mut file, &mut protected)?;
        let mut trailing = [0_u8; 1];
        if file.read(&mut trailing)? != 0 {
            return Err(CredentialStoreError::InvalidProtectedKeyFile);
        }
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

    #[test]
    fn saving_an_existing_master_key_never_replaces_it() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("immutable-master-key.bin")?;
        let key_file = ProtectedMasterKeyFile::new(&path);
        key_file.save(&[1, 2, 3])?;

        assert!(!key_file.save_if_absent(&[4, 5, 6])?);
        assert!(key_file.save(&[4, 5, 6]).is_err());
        assert_eq!(key_file.load()?, vec![1, 2, 3]);
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn concurrent_master_key_creators_publish_exactly_one_complete_key()
    -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("concurrent-master-key.bin")?;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (1_u8..=8)
            .map(|value| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let published =
                        ProtectedMasterKeyFile::new(path).save_if_absent(&[value; 4096]);
                    (value, published)
                })
            })
            .collect();
        let mut winner = None;
        for worker in workers {
            let (value, published) = worker.join().map_err(|_| CredentialStoreError::IoFailed)?;
            if published? {
                assert!(winner.replace(value).is_none());
            }
        }
        assert_eq!(
            ProtectedMasterKeyFile::new(&path).load()?,
            vec![winner.ok_or(CredentialStoreError::IoFailed)?; 4096]
        );
        fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn corrupted_key_length_is_bounded_before_allocation() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("oversized-master-key.bin")?;
        let mut bytes = PROTECTED_MASTER_KEY_MAGIC.to_vec();
        bytes.push(PROTECTED_MASTER_KEY_VERSION);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, bytes)?;
        assert_eq!(
            ProtectedMasterKeyFile::new(&path).load(),
            Err(CredentialStoreError::InvalidProtectedKeyFile)
        );
        fs::remove_file(path)?;
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
