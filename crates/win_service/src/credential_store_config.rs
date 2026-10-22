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
    pinned: bool,
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
    pub fn from_environment_or_default() -> Result<Self, ProtocolError> {
        if let Some(store_dir) = credential_store::location::load_pinned_store_directory()
            .map_err(|_| ProtocolError::TransportUnavailable)?
        {
            return Ok(Self {
                pinned: true,
                ..Self::from_store_dir(store_dir)
            });
        }
        if let Some(store_dir) = std::env::var_os(ENV_CREDENTIAL_STORE_DIR) {
            let store_dir = PathBuf::from(store_dir);
            if !store_dir.is_absolute() {
                return Err(ProtocolError::InvalidMessage);
            }
            return Ok(Self::from_store_dir(store_dir));
        }

        let root = std::env::current_exe()
            .map_err(|_| ProtocolError::TransportUnavailable)?
            .parent()
            .map(|parent| parent.join(STORE_DIR_NAME))
            .ok_or(ProtocolError::TransportUnavailable)?;
        Ok(Self::from_store_dir(root))
    }

    pub fn from_store_dir(store_dir: PathBuf) -> Self {
        Self {
            master_key_path: store_dir.join(MASTER_KEY_FILE_NAME),
            database_path: store_dir.join(DATABASE_FILE_NAME),
            pinned: false,
        }
    }
}

pub fn open_service_credential_store(
    paths: &ServiceCredentialStorePaths,
) -> Result<ServiceCredentialStoreContext, ProtocolError> {
    // A missing half of an existing store must never become a fresh store.
    // In particular, generating another key cannot recover an encrypted database.
    credential_store_files_exist(paths)?;
    ensure_parent_directory(&paths.master_key_path)?;
    ensure_parent_directory(&paths.database_path)?;
    let master_key = load_or_create_service_master_key(&paths.master_key_path)?;
    let repository = SqlCipherRepository::open(&paths.database_path, &master_key)
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    let mut store = RepositoryCredentialStore::new(repository);
    store
        .initialize(&HardwareFingerprint::empty())
        .map_err(credential_store_error_to_protocol_error)?;
    if paths.pinned {
        let directory = paths
            .database_path
            .parent()
            .ok_or(ProtocolError::InvalidMessage)?;
        credential_store::location::mark_pinned_store_initialized(directory)
            .map_err(|_| ProtocolError::TransportUnavailable)?;
    }
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
    save_enrollment(&mut store, &master_key, enrollment)?;
    // Only an explicit update of the installed store may re-enable automatic
    // Windows submission. Temporary diagnostics/test stores must not touch the
    // machine guard. Neither checking/decrypting a credential nor restarting
    // the service clears a Windows rejection.
    if paths.pinned {
        use logon_safety::LogonSubmissionGuard;
        logon_safety::PersistentLogonGuard
            .credentials_updated()
            .map_err(|_| ProtocolError::TransportUnavailable)?;
    }
    Ok(())
}

pub fn is_credential_secret_configured(
    paths: &ServiceCredentialStorePaths,
    user_id: &UserId,
    credential_ref: &CredentialRef,
) -> Result<bool, ProtocolError> {
    if !credential_store_files_exist(paths)? {
        return Ok(false);
    }

    let ServiceCredentialStoreContext { store, master_key } = open_service_credential_store(paths)?;
    let user = match store.get_user(user_id) {
        Ok(user) => user,
        Err(CredentialStoreError::UserNotFound) => return Ok(false),
        Err(error) => return Err(credential_store_error_to_protocol_error(error)),
    };
    if user.credential_ref != *credential_ref {
        return Ok(false);
    }
    // A user record referencing a missing/corrupt blob is unavailable, not an
    // unenrolled account. Verify decryption as well as the serialized blob.
    let blob = store
        .load_encrypted_credential_blob(credential_ref)
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    AesGcmCredentialBlobProtector::new(WindowsSecureRandom::new())
        .decrypt_credential_secret(
            &master_key,
            &CredentialBlobAssociatedData {
                user_id: user.user_id,
                credential_ref: user.credential_ref,
            },
            &blob,
        )
        .map_err(|_| ProtocolError::TransportUnavailable)?;
    Ok(true)
}

pub fn ensure_development_credential_if_missing(
    store: &mut ServiceCredentialStore,
    master_key: &MasterKey,
    user_id: &UserId,
) -> Result<(), ProtocolError> {
    match store.get_user(user_id) {
        Ok(_) => return Ok(()),
        Err(CredentialStoreError::UserNotFound) => {}
        Err(error) => return Err(credential_store_error_to_protocol_error(error)),
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
        .save_credential_enrollment(
            UserRecord {
                user_id: enrollment.user_id,
                user_sid: enrollment.user_sid,
                username: enrollment.username,
                account_type: enrollment.account_type,
                credential_ref: enrollment.credential_ref,
            },
            credential_blob,
        )
        .map_err(credential_store_error_to_protocol_error)
}

fn load_or_create_service_master_key(master_key_path: &Path) -> Result<MasterKey, ProtocolError> {
    let key_file = ProtectedMasterKeyFile::new(master_key_path.to_path_buf());
    let key_protector = WindowsDpapiKeyProtector::new();

    if key_file
        .path()
        .try_exists()
        .map_err(|_| ProtocolError::TransportUnavailable)?
    {
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
        if key_file
            .save_if_absent(&protected)
            .map_err(|_| ProtocolError::TransportUnavailable)?
        {
            Ok(master_key)
        } else {
            let protected = key_file
                .load()
                .map_err(|_| ProtocolError::TransportUnavailable)?;
            key_protector
                .unprotect_master_key(&protected)
                .map_err(|_| ProtocolError::TransportUnavailable)
        }
    }
}

fn credential_store_files_exist(
    paths: &ServiceCredentialStorePaths,
) -> Result<bool, ProtocolError> {
    if paths.pinned {
        // Paths can outlive their original lookup. Revalidate the initialization
        // marker and pin on every access before considering file creation.
        let directory = credential_store::location::load_pinned_store_directory()
            .map_err(|_| ProtocolError::TransportUnavailable)?
            .ok_or(ProtocolError::TransportUnavailable)?;
        if paths.master_key_path != directory.join(MASTER_KEY_FILE_NAME)
            || paths.database_path != directory.join(DATABASE_FILE_NAME)
        {
            return Err(ProtocolError::TransportUnavailable);
        }
    }
    let is_file = |path: &Path| match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(ProtocolError::TransportUnavailable),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ProtocolError::TransportUnavailable),
    };
    match (
        is_file(&paths.master_key_path)?,
        is_file(&paths.database_path)?,
    ) {
        (true, true) => Ok(true),
        (false, false) => Ok(false),
        _ => Err(ProtocolError::TransportUnavailable),
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

    fn unique_store_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "winfaceunlock-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn missing_master_key_never_replaces_an_existing_database() -> std::io::Result<()> {
        let root = unique_store_root("missing-key");
        let paths = ServiceCredentialStorePaths::from_store_dir(root.clone());
        std::fs::create_dir_all(&root)?;
        std::fs::write(&paths.database_path, b"existing encrypted database")?;

        assert!(matches!(
            open_service_credential_store(&paths),
            Err(ProtocolError::TransportUnavailable)
        ));
        assert_eq!(
            is_credential_secret_configured(
                &paths,
                &UserId("user".into()),
                &CredentialRef("credential".into())
            ),
            Err(ProtocolError::TransportUnavailable)
        );
        assert!(!paths.master_key_path.exists());
        assert_eq!(
            std::fs::read(&paths.database_path)?,
            b"existing encrypted database"
        );
        std::fs::remove_dir_all(root)
    }

    #[test]
    fn missing_database_never_creates_an_empty_replacement() -> std::io::Result<()> {
        let root = unique_store_root("missing-database");
        let paths = ServiceCredentialStorePaths::from_store_dir(root.clone());
        std::fs::create_dir_all(&root)?;
        std::fs::write(&paths.master_key_path, b"existing protected key")?;

        assert!(matches!(
            open_service_credential_store(&paths),
            Err(ProtocolError::TransportUnavailable)
        ));
        assert_eq!(
            is_credential_secret_configured(
                &paths,
                &UserId("user".into()),
                &CredentialRef("credential".into())
            ),
            Err(ProtocolError::TransportUnavailable)
        );
        assert!(!paths.database_path.exists());
        assert_eq!(
            std::fs::read(&paths.master_key_path)?,
            b"existing protected key"
        );
        std::fs::remove_dir_all(root)
    }

    #[test]
    fn corrupted_credential_is_unavailable_instead_of_not_configured() -> Result<(), ProtocolError>
    {
        let root = unique_store_root("corrupt-credential");
        let paths = ServiceCredentialStorePaths::from_store_dir(root.clone());
        let user_id = UserId("user".into());
        let credential_ref = CredentialRef("credential".into());
        enroll_windows_credential(
            &paths,
            WindowsCredentialEnrollment {
                user_id: user_id.clone(),
                user_sid: "S-1-5-test".into(),
                username: "test".into(),
                account_type: AccountType::Local,
                credential_ref: credential_ref.clone(),
                password: "test-secret".into(),
            },
        )?;
        let ServiceCredentialStoreContext { mut store, .. } =
            open_service_credential_store(&paths)?;
        store
            .save_encrypted_credential_blob(
                credential_ref.clone(),
                credential_store::CredentialBlob::new(
                    credential_store::CredentialBlobAlgorithm::Aes256GcmV1,
                    vec![0; 12],
                    vec![0; 16],
                ),
            )
            .map_err(credential_store_error_to_protocol_error)?;
        drop(store);
        assert_eq!(
            is_credential_secret_configured(&paths, &user_id, &credential_ref),
            Err(ProtocolError::TransportUnavailable)
        );
        std::fs::remove_dir_all(root).map_err(|_| ProtocolError::TransportUnavailable)?;
        Ok(())
    }

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
