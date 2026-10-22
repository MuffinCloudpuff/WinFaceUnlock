//! Persistent storage location, independent of executable upgrades and auth settings.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::CredentialStoreError;

mod registry;

pub const STORE_DIRECTORY_NAME: &str = "credential-store";
pub const MASTER_KEY_FILE_NAME: &str = "protected-master-key.bin";
pub const DATABASE_FILE_NAME: &str = "credential-store.db";

pub fn load_pinned_store_directory() -> Result<Option<PathBuf>, CredentialStoreError> {
    let path = registry::load()?;
    if let Some(path) = &path {
        validate_store_directory(path, !registry::initialized()?)?;
    }
    Ok(path)
}

/// Installer-only operation. Existing pins always win, including after a
/// preserve-data uninstall; no implicit migration or merging of stores occurs.
pub fn pin_store_directory(path: &Path) -> Result<PathBuf, CredentialStoreError> {
    if let Some(pinned) = load_pinned_store_directory()? {
        return Ok(pinned);
    }
    validate_store_directory(path, true)?;
    registry::save(path, path.join(DATABASE_FILE_NAME).try_exists()?)?;
    Ok(path.to_path_buf())
}

pub fn mark_pinned_store_initialized(path: &Path) -> Result<(), CredentialStoreError> {
    if registry::load()?.as_deref() != Some(path) {
        return Err(CredentialStoreError::StoreUnavailable);
    }
    validate_store_directory(path, false)?;
    registry::mark_initialized()
}

pub fn legacy_store_directory() -> Result<Option<PathBuf>, CredentialStoreError> {
    let path = registry::legacy_directory()?;
    if let Some(path) = &path {
        validate_absolute_directory(path)?;
    }
    Ok(path)
}

pub fn validate_store_directory(path: &Path, allow_new: bool) -> Result<(), CredentialStoreError> {
    validate_absolute_directory(path)?;
    let exists = |name: &str| -> Result<bool, CredentialStoreError> {
        match fs::metadata(path.join(name)) {
            Ok(metadata) if metadata.is_file() => Ok(true),
            Ok(_) => Err(CredentialStoreError::StoreUnavailable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    };
    match (exists(MASTER_KEY_FILE_NAME)?, exists(DATABASE_FILE_NAME)?) {
        (true, true) => Ok(()),
        (false, false) if allow_new => Ok(()),
        _ => Err(CredentialStoreError::StoreUnavailable),
    }
}

fn validate_absolute_directory(path: &Path) -> Result<(), CredentialStoreError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(CredentialStoreError::RecordValidationFailed);
    }
    Ok(())
}

/// Called only by explicit delete-data uninstall. Delete the exact known data
/// files rather than recursively deleting a path read from the registry.
pub fn delete_pinned_store_data() -> Result<(), CredentialStoreError> {
    if let Some(path) = registry::load()? {
        validate_absolute_directory(&path)?;
        for name in [
            MASTER_KEY_FILE_NAME,
            DATABASE_FILE_NAME,
            "credential-store.db-wal",
            "credential-store.db-shm",
            "credential-store.db-journal",
        ] {
            match fs::remove_file(path.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    registry::clear()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_rejects_relative_or_partially_missing_store() -> Result<(), CredentialStoreError> {
        assert_eq!(
            validate_store_directory(Path::new("relative-store"), true),
            Err(CredentialStoreError::RecordValidationFailed)
        );
        let root = std::env::temp_dir().join(format!(
            "wfu-store-location-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| CredentialStoreError::IoFailed)?
                .as_nanos()
        ));
        fs::create_dir_all(&root)?;
        assert!(validate_store_directory(&root, true).is_ok());
        assert!(validate_store_directory(&root, false).is_err());
        fs::write(root.join(MASTER_KEY_FILE_NAME), b"protected-test-key")?;
        assert!(validate_store_directory(&root, true).is_err());
        fs::write(root.join(DATABASE_FILE_NAME), b"encrypted-test-database")?;
        assert!(validate_store_directory(&root, false).is_ok());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
