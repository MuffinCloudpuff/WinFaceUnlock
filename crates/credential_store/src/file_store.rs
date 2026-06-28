use std::collections::HashMap;

use common_protocol::{CredentialRef, ProtectedCredential, UserId};
use hardware_binding::HardwareFingerprint;

use crate::CredentialBlob;
use crate::{CredentialStoreError, KeyProtector, MasterKey, ProtectedMasterKeyFile, UserRecord};

pub trait CredentialStore {
    fn initialize(&mut self, fingerprint: &HardwareFingerprint)
    -> Result<(), CredentialStoreError>;
    fn upsert_user(&mut self, record: UserRecord) -> Result<(), CredentialStoreError>;
    fn get_user(&self, user_id: &UserId) -> Result<UserRecord, CredentialStoreError>;
    fn get_protected_credential(
        &self,
        user_id: &UserId,
    ) -> Result<ProtectedCredential, CredentialStoreError>;
    fn save_encrypted_credential_blob(
        &mut self,
        credential_ref: CredentialRef,
        credential_blob: CredentialBlob,
    ) -> Result<(), CredentialStoreError>;
    fn load_encrypted_credential_blob(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialBlob, CredentialStoreError>;
}

pub struct FileCredentialStore<P: KeyProtector> {
    key_file: ProtectedMasterKeyFile,
    key_protector: P,
    users: HashMap<UserId, UserRecord>,
    encrypted_credential_blobs: HashMap<CredentialRef, CredentialBlob>,
    master_key: Option<MasterKey>,
    fingerprint: Option<HardwareFingerprint>,
}

impl<P: KeyProtector> FileCredentialStore<P> {
    pub fn new(key_file: ProtectedMasterKeyFile, key_protector: P) -> Self {
        Self {
            key_file,
            key_protector,
            users: HashMap::new(),
            encrypted_credential_blobs: HashMap::new(),
            master_key: None,
            fingerprint: None,
        }
    }

    pub fn is_master_key_loaded_for_current_session(&self) -> bool {
        self.master_key.is_some()
    }

    fn load_or_create_master_key(&self) -> Result<MasterKey, CredentialStoreError> {
        if self.key_file.path().exists() {
            let protected = self.key_file.load()?;
            self.key_protector.unprotect_master_key(&protected)
        } else {
            let master_key = self.key_protector.generate_master_key()?;
            let protected = self.key_protector.protect_master_key(&master_key)?;
            self.key_file.save(&protected)?;
            Ok(master_key)
        }
    }
}

impl<P: KeyProtector> CredentialStore for FileCredentialStore<P> {
    fn initialize(
        &mut self,
        fingerprint: &HardwareFingerprint,
    ) -> Result<(), CredentialStoreError> {
        let master_key = self.load_or_create_master_key()?;
        self.master_key = Some(master_key);
        self.fingerprint = Some(fingerprint.clone());
        Ok(())
    }

    fn upsert_user(&mut self, record: UserRecord) -> Result<(), CredentialStoreError> {
        self.users.insert(record.user_id.clone(), record);
        Ok(())
    }

    fn get_user(&self, user_id: &UserId) -> Result<UserRecord, CredentialStoreError> {
        self.users
            .get(user_id)
            .cloned()
            .ok_or(CredentialStoreError::UserNotFound)
    }

    fn get_protected_credential(
        &self,
        user_id: &UserId,
    ) -> Result<ProtectedCredential, CredentialStoreError> {
        let user = self.get_user(user_id)?;
        Ok(ProtectedCredential {
            user_id: user.user_id,
            credential_ref: user.credential_ref,
        })
    }

    fn save_encrypted_credential_blob(
        &mut self,
        credential_ref: CredentialRef,
        credential_blob: CredentialBlob,
    ) -> Result<(), CredentialStoreError> {
        self.encrypted_credential_blobs
            .insert(credential_ref, credential_blob);
        Ok(())
    }

    fn load_encrypted_credential_blob(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<CredentialBlob, CredentialStoreError> {
        self.encrypted_credential_blobs
            .get(credential_ref)
            .cloned()
            .ok_or(CredentialStoreError::CredentialNotFound)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use common_protocol::{AccountType, CredentialRef, UserId};
    use hardware_binding::HardwareFingerprint;

    use crate::{
        CredentialBlob, CredentialBlobAlgorithm, CredentialStore, CredentialStoreError,
        FileCredentialStore, ProtectedMasterKeyFile, UserRecord,
        key_protector::tests::MemoryKeyProtector,
    };

    #[test]
    fn file_store_initializes_and_loads_master_key() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("store-master-key.bin")?;
        let key_file = ProtectedMasterKeyFile::new(&path);
        let mut store = FileCredentialStore::new(key_file, MemoryKeyProtector);

        store.initialize(&HardwareFingerprint::empty())?;
        let first_key_loaded = store.is_master_key_loaded_for_current_session();

        let key_file = ProtectedMasterKeyFile::new(&path);
        let mut second_store = FileCredentialStore::new(key_file, MemoryKeyProtector);
        second_store.initialize(&HardwareFingerprint::empty())?;
        let second_key_loaded = second_store.is_master_key_loaded_for_current_session();
        let _ = fs::remove_file(path);

        assert!(first_key_loaded);
        assert!(second_key_loaded);
        Ok(())
    }

    #[test]
    fn file_store_keeps_encrypted_credential_blob_by_ref() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("blob-master-key.bin")?;
        let key_file = ProtectedMasterKeyFile::new(&path);
        let mut store = FileCredentialStore::new(key_file, MemoryKeyProtector);
        let credential_ref = CredentialRef("cred-1".to_owned());
        let credential_blob = CredentialBlob::new(
            CredentialBlobAlgorithm::Aes256GcmV1,
            vec![1; 12],
            vec![2; 16],
        );

        store.save_encrypted_credential_blob(credential_ref.clone(), credential_blob.clone())?;
        let loaded_blob = store.load_encrypted_credential_blob(&credential_ref)?;
        let _ = fs::remove_file(path);

        assert_eq!(loaded_blob, credential_blob);
        Ok(())
    }

    #[test]
    fn file_store_returns_protected_credential_reference() -> Result<(), CredentialStoreError> {
        let path = unique_temp_path("users-master-key.bin")?;
        let key_file = ProtectedMasterKeyFile::new(&path);
        let mut store = FileCredentialStore::new(key_file, MemoryKeyProtector);
        let user_id = UserId("user-1".to_owned());

        store.initialize(&HardwareFingerprint::empty())?;
        store.upsert_user(UserRecord {
            user_id: user_id.clone(),
            user_sid: "S-1-5-21-example".to_owned(),
            username: "Liu".to_owned(),
            account_type: AccountType::Local,
            credential_ref: CredentialRef("cred-1".to_owned()),
        })?;

        let protected_credential = store.get_protected_credential(&user_id)?;
        let _ = fs::remove_file(path);

        assert_eq!(
            protected_credential.credential_ref,
            CredentialRef("cred-1".to_owned())
        );
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
