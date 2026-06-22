use std::path::{Path, PathBuf};

use common_protocol::{AccountType, CredentialRef, ProtocolError, UserId};
use credential_store::{
    AesGcmCredentialBlobProtector, CredentialBlobAssociatedData, CredentialBlobProtector,
    CredentialSecret, CredentialStore, CredentialStoreError, KeyProtector, MasterKey,
    ProtectedMasterKeyFile, RepositoryCredentialStore, SqlCipherRepository, UserRecord,
    WindowsDpapiKeyProtector, WindowsSecureRandom,
};
use hardware_binding::HardwareFingerprint;

pub const ENV_CREDENTIAL_STORE_DIR: &str = "WINFACEUNLOCK_STORE_DIR";
const STORE_DIR_NAME: &str = "credential-store";
const MASTER_KEY_FILE_NAME: &str = "protected-master-key.bin";
const DATABASE_FILE_NAME: &str = "credential-store.db";
const DEFAULT_POLICY_USER_SID: &str = "S-1-5-21-winfaceunlock-pending";

pub type ServiceCredentialStore = RepositoryCredentialStore<SqlCipherRepository>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceCredentialStorePaths {
    pub master_key_path: PathBuf,
    pub database_path: PathBuf,
}

pub struct ServiceCredentialStoreContext {
    pub store: ServiceCredentialStore,
    pub master_key: MasterKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsCredentialEnrollment {
    pub user_id: UserId,
    pub user_sid: String,
    pub username: String,
    pub account_type: AccountType,
    pub credential_ref: CredentialRef,
    pub password: String,
}

impl ServiceCredentialStorePaths {
    pub fn from_environment_or_default() -> Self {
        if let Some(store_dir) = std::env::var_os(ENV_CREDENTIAL_STORE_DIR) {
            return Self::from_store_dir(PathBuf::from(store_dir));
        }

        let root = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join(STORE_DIR_NAME)))
            .unwrap_or_else(|| {
                std::env::temp_dir()
                    .join("WinFaceUnlock")
                    .join(STORE_DIR_NAME)
            });
        Self::from_store_dir(root)
    }

    pub fn from_store_dir(store_dir: PathBuf) -> Self {
        Self {
            master_key_path: store_dir.join(MASTER_KEY_FILE_NAME),
            database_path: store_dir.join(DATABASE_FILE_NAME),
        }
    }
}

pub fn open_service_credential_store(
    paths: &ServiceCredentialStorePaths,
) -> Result<ServiceCredentialStoreContext, ProtocolError> {
    ensure_parent_directory(&paths.master_key_path)?;
    ensure_parent_directory(&paths.database_path)?;
    let master_key = load_or_create_service_master_key(&paths.master_key_path)?;
    let repository = SqlCipherRepository::open(&paths.database_path, &master_key)
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    let mut store = RepositoryCredentialStore::new(repository);
    store
        .initialize(&HardwareFingerprint::empty())
        .map_err(credential_store_error_to_protocol_error)?;
    Ok(ServiceCredentialStoreContext { store, master_key })
}

pub fn enroll_windows_credential(
    paths: &ServiceCredentialStorePaths,
    enrollment: WindowsCredentialEnrollment,
) -> Result<(), ProtocolError> {
    let ServiceCredentialStoreContext {
        mut store,
        master_key,
    } = open_service_credential_store(paths)?;
    save_enrollment(&mut store, &master_key, enrollment)
}

pub fn is_credential_secret_configured(
    paths: &ServiceCredentialStorePaths,
    user_id: &UserId,
    credential_ref: &CredentialRef,
) -> Result<bool, ProtocolError> {
    if !paths.master_key_path.is_file() || !paths.database_path.is_file() {
        return Ok(false);
    }

    let ServiceCredentialStoreContext { store, .. } = open_service_credential_store(paths)?;
    let configured = match (
        store.get_user(user_id),
        store.load_encrypted_credential_blob(credential_ref),
    ) {
        (Ok(user), Ok(_)) if user.credential_ref == *credential_ref => true,
        (Err(CredentialStoreError::UserNotFound), _)
        | (_, Err(CredentialStoreError::CredentialNotFound))
        | (Ok(_), Ok(_)) => false,
        (Err(error), _) | (_, Err(error)) => {
            return Err(credential_store_error_to_protocol_error(error));
        }
    };
    Ok(configured)
}

pub fn ensure_development_credential_if_missing(
    store: &mut ServiceCredentialStore,
    master_key: &MasterKey,
    user_id: &UserId,
) -> Result<(), ProtocolError> {
    if store.get_user(user_id).is_ok() {
        return Ok(());
    }

    save_enrollment(
        store,
        master_key,
        WindowsCredentialEnrollment {
            user_id: user_id.clone(),
            user_sid: DEFAULT_POLICY_USER_SID.to_owned(),
            username: "dev-user".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef("dev-credential-ref".to_owned()),
            password: std::env::var("WINFACEUNLOCK_DEV_PASSWORD")
                .unwrap_or_else(|_| "dev-password".to_owned()),
        },
    )
}

fn save_enrollment(
    store: &mut ServiceCredentialStore,
    master_key: &MasterKey,
    enrollment: WindowsCredentialEnrollment,
) -> Result<(), ProtocolError> {
    let credential_blob = AesGcmCredentialBlobProtector::new(WindowsSecureRandom::new())
        .encrypt_credential_secret(
            master_key,
            &CredentialBlobAssociatedData {
                user_id: enrollment.user_id.clone(),
                credential_ref: enrollment.credential_ref.clone(),
            },
            &CredentialSecret::from_utf8_password(enrollment.password),
        )
        .map_err(credential_store_error_to_protocol_error)?;

    store
        .save_encrypted_credential_blob(enrollment.credential_ref.clone(), credential_blob)
        .map_err(credential_store_error_to_protocol_error)?;
    store
        .upsert_user(UserRecord {
            user_id: enrollment.user_id,
            user_sid: enrollment.user_sid,
            username: enrollment.username,
            account_type: enrollment.account_type,
            credential_ref: enrollment.credential_ref,
        })
        .map_err(credential_store_error_to_protocol_error)
}

fn load_or_create_service_master_key(master_key_path: &Path) -> Result<MasterKey, ProtocolError> {
    let key_file = ProtectedMasterKeyFile::new(master_key_path.to_path_buf());
    let key_protector = WindowsDpapiKeyProtector::new();

    if key_file.path().exists() {
        let protected = key_file
            .load()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        key_protector
            .unprotect_master_key(&protected)
            .map_err(|_| ProtocolError::TransportUnavailable)
    } else {
        let master_key = key_protector
            .generate_master_key()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let protected = key_protector
            .protect_master_key(&master_key)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        key_file
            .save(&protected)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        Ok(master_key)
    }
}

fn ensure_parent_directory(path: &Path) -> Result<(), ProtocolError> {
    let parent = path.parent().ok_or(ProtocolError::InvalidMessage)?;
    std::fs::create_dir_all(parent).map_err(|_| ProtocolError::TransportUnavailable)
}

fn credential_store_error_to_protocol_error(error: CredentialStoreError) -> ProtocolError {
    match error {
        CredentialStoreError::UserNotFound | CredentialStoreError::CredentialNotFound => {
            ProtocolError::Unauthorized
        }
        CredentialStoreError::StoreUnavailable
        | CredentialStoreError::RepositoryUnavailable
        | CredentialStoreError::IoFailed => ProtocolError::TransportUnavailable,
        _ => ProtocolError::InvalidMessage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_paths_use_named_files_under_store_dir() {
        let paths = ServiceCredentialStorePaths::from_store_dir(PathBuf::from(r"C:\Store"));

        assert_eq!(
            paths.master_key_path,
            PathBuf::from(r"C:\Store\protected-master-key.bin")
        );
        assert_eq!(
            paths.database_path,
            PathBuf::from(r"C:\Store\credential-store.db")
        );
    }

    #[test]
    fn development_credential_does_not_overwrite_existing_user() -> Result<(), ProtocolError> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-store-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let paths = ServiceCredentialStorePaths::from_store_dir(root.clone());
        let user_id = UserId("dev-user".to_owned());
        enroll_windows_credential(
            &paths,
            WindowsCredentialEnrollment {
                user_id: user_id.clone(),
                user_sid: "S-1-5-real".to_owned(),
                username: "Leo16".to_owned(),
                account_type: AccountType::Local,
                credential_ref: CredentialRef("real-cred".to_owned()),
                password: "secret".to_owned(),
            },
        )?;
        let ServiceCredentialStoreContext {
            mut store,
            master_key,
        } = open_service_credential_store(&paths)?;

        ensure_development_credential_if_missing(&mut store, &master_key, &user_id)?;
        let user = store
            .get_user(&user_id)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(user.username, "Leo16");
        assert_eq!(user.credential_ref, CredentialRef("real-cred".to_owned()));
        Ok(())
    }

    #[test]
    fn credential_secret_configured_state_tracks_user_and_blob() -> Result<(), ProtocolError> {
        let root = std::env::temp_dir().join(format!(
            "winfaceunlock-store-state-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let paths = ServiceCredentialStorePaths::from_store_dir(root.clone());
        let user_id = UserId("dev-user".to_owned());
        let credential_ref = CredentialRef("real-cred".to_owned());

        assert!(!is_credential_secret_configured(
            &paths,
            &user_id,
            &credential_ref
        )?);

        enroll_windows_credential(
            &paths,
            WindowsCredentialEnrollment {
                user_id: user_id.clone(),
                user_sid: "S-1-5-real".to_owned(),
                username: "Leo16".to_owned(),
                account_type: AccountType::Local,
                credential_ref: credential_ref.clone(),
                password: "secret".to_owned(),
            },
        )?;

        assert!(is_credential_secret_configured(
            &paths,
            &user_id,
            &credential_ref
        )?);

        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }
}
